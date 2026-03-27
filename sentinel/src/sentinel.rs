use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use git2::Repository;
use std::fmt;
use std::path::Path;

pub const SENTINEL_BRANCH: &str = "claude-deploy-sentinels";

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    New,
    Claiming,
    Running,
    Success,
    Failure,
    Abandoned,
    Unknown(String),
}

impl Status {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "new"       => Self::New,
            "claiming"  => Self::Claiming,
            "running"   => Self::Running,
            "success"   => Self::Success,
            "failure"   => Self::Failure,
            "abandoned" => Self::Abandoned,
            other       => Self::Unknown(other.to_string()),
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failure | Self::Abandoned)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::New | Self::Claiming | Self::Running)
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::New       => "⏳",
            Self::Claiming  => "🔒",
            Self::Running   => "⚡",
            Self::Success   => "✅",
            Self::Failure   => "❌",
            Self::Abandoned => "💀",
            Self::Unknown(_)=> "❓",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::New        => "new",
            Self::Claiming   => "claiming",
            Self::Running    => "running",
            Self::Success    => "success",
            Self::Failure    => "failure",
            Self::Abandoned  => "abandoned",
            Self::Unknown(s) => s.as_str(),
        };
        write!(f, "{}", s)
    }
}

// ---------------------------------------------------------------------------
// Sentinel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Sentinel {
    pub name: String,
    pub status: Status,
    pub main_ref: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub ran: Option<DateTime<Utc>>,
    pub completed: Option<DateTime<Utc>>,
    pub worker: Option<String>,
    pub capture: Option<String>,
    pub msg: Option<String>,
    pub result_ref: Option<String>,
    pub script_body: String,
    pub log: Option<String>,
}

impl Sentinel {
    /// Parse a sentinel file from its raw text content.
    pub fn parse(name: impl Into<String>, content: &str) -> Self {
        let name = name.into();
        let mut status = Status::Unknown("".into());
        let mut main_ref = None;
        let mut created = None;
        let mut ran = None;
        let mut completed = None;
        let mut worker = None;
        let mut capture = None;
        let mut msg = None;
        let mut result_ref = None;
        let mut script_lines = Vec::new();
        let mut log_lines = Vec::new();
        let mut in_body = false;
        let mut in_log = false;

        for line in content.lines() {
            if in_log {
                // Strip leading "# " prefix added by watcher
                let stripped = line.strip_prefix("# ").unwrap_or(line);
                log_lines.push(stripped.to_string());
                continue;
            }
            if in_body {
                if line == "# --- log ---" || line == "# --- abandoned ---" {
                    in_log = true;
                    continue;
                }
                script_lines.push(line.to_string());
                continue;
            }
            if line.is_empty() {
                in_body = true;
                continue;
            }
            if let Some(val) = field(line, "status")     { status = Status::from_str(val); }
            if let Some(val) = field(line, "main-ref")   { main_ref = Some(val.to_string()); }
            if let Some(val) = field(line, "created")    { created = parse_dt(val); }
            if let Some(val) = field(line, "ran")        { ran = parse_dt(val); }
            if let Some(val) = field(line, "completed")  { completed = parse_dt(val); }
            if let Some(val) = field(line, "worker")     { worker = Some(val.to_string()); }
            if let Some(val) = field(line, "capture")    { capture = Some(val.to_string()); }
            if let Some(val) = field(line, "msg")        { msg = Some(val.to_string()); }
            if let Some(val) = field(line, "result-ref") { result_ref = Some(val.to_string()); }
        }

        let log = if log_lines.is_empty() {
            None
        } else {
            Some(log_lines.join("\n"))
        };

        Self {
            name,
            status,
            main_ref,
            created,
            ran,
            completed,
            worker,
            capture,
            msg,
            result_ref,
            script_body: script_lines.join("\n"),
            log,
        }
    }

    /// Age of the sentinel since `ran` (or `created`) timestamp, in seconds.
    pub fn age_secs(&self) -> Option<i64> {
        let ts = self.ran.or(self.created)?;
        Some((Utc::now() - ts).num_seconds())
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

/// Read all sentinels from origin/claude-deploy-sentinels without checking out.
/// Returns sentinels sorted by filename (chronological by timestamp in name).
pub fn read_all(repo: &Repository) -> Result<Vec<Sentinel>> {
    fetch_sentinel_branch(repo)?;

    let branch_ref = format!("refs/remotes/origin/{}", SENTINEL_BRANCH);
    let obj = repo
        .revparse_single(&branch_ref)
        .with_context(|| format!("sentinel branch not found: {}", branch_ref))?;

    let commit = obj.peel_to_commit()?;
    let tree = commit.tree()?;

    let mut sentinels = Vec::new();

    tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
        let name = entry.name().unwrap_or("").to_string();
        if !name.starts_with("run-") {
            return git2::TreeWalkResult::Ok;
        }

        let blob = match entry
            .to_object(repo)
            .and_then(|o| o.peel_to_blob())
        {
            Ok(b) => b,
            Err(_) => return git2::TreeWalkResult::Ok,
        };

        let content = match std::str::from_utf8(blob.content()) {
            Ok(s) => s.to_string(),
            Err(_) => return git2::TreeWalkResult::Ok,
        };

        sentinels.push(Sentinel::parse(name, &content));
        git2::TreeWalkResult::Ok
    })?;

    sentinels.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sentinels)
}

/// Fetch origin/claude-deploy-sentinels quietly.
pub fn fetch_sentinel_branch(repo: &Repository) -> Result<()> {
    let mut remote = repo
        .find_remote("origin")
        .context("no 'origin' remote")?;

    remote
        .fetch(&[SENTINEL_BRANCH], None, None)
        .context("fetch sentinel branch")?;

    Ok(())
}

/// Generate a unique sentinel filename.
/// Format: run-<ref8>-<YYYYMMDDTHHmmss>-<rand4>
pub fn new_name(repo: &Repository) -> Result<String> {
    let head = repo.head()?.peel_to_commit()?;
    let ref8 = &head.id().to_string()[..8];
    let ts = Utc::now().format("%Y%m%dT%H%M%S");
    let rand = rand4();
    Ok(format!("run-{}-{}-{}", ref8, ts, rand))
}

fn rand4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Simple: xor pid + nanos, format as 4 hex chars
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{:04x}", (nanos ^ pid) & 0xffff)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}:", key);
    line.strip_prefix(&prefix).map(|s| s.trim())
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 with T separator
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            // Try without timezone (assume UTC): 2026-03-27T03:46:00
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}
