use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::fmt;
use std::path::Path;
use std::process::Command;

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
            Self::New        => "⏳",
            Self::Claiming   => "🔒",
            Self::Running    => "⚡",
            Self::Success    => "✅",
            Self::Failure    => "❌",
            Self::Abandoned  => "💀",
            Self::Unknown(_) => "❓",
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
        let mut script_lines: Vec<String> = Vec::new();
        let mut log_lines: Vec<String> = Vec::new();
        let mut in_body = false;
        let mut in_log = false;

        for line in content.lines() {
            if in_log {
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
            if let Some(v) = field(line, "status")     { status = Status::from_str(v); }
            if let Some(v) = field(line, "main-ref")   { main_ref = Some(v.to_string()); }
            if let Some(v) = field(line, "created")    { created = parse_dt(v); }
            if let Some(v) = field(line, "ran")        { ran = parse_dt(v); }
            if let Some(v) = field(line, "completed")  { completed = parse_dt(v); }
            if let Some(v) = field(line, "worker")     { worker = Some(v.to_string()); }
            if let Some(v) = field(line, "capture")    { capture = Some(v.to_string()); }
            if let Some(v) = field(line, "msg")        { msg = Some(v.to_string()); }
            if let Some(v) = field(line, "result-ref") { result_ref = Some(v.to_string()); }
        }

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
            log: if log_lines.is_empty() { None } else { Some(log_lines.join("\n")) },
        }
    }

    pub fn age_secs(&self) -> Option<i64> {
        let ts = self.ran.or(self.created)?;
        Some((Utc::now() - ts).num_seconds())
    }
}

// ---------------------------------------------------------------------------
// Git shell helpers
// Shell out for network ops (fetch/push) — works with both git and jj repos.
// Read ops use gix for speed (no subprocess overhead).
// ---------------------------------------------------------------------------

/// Fetch origin/claude-deploy-sentinels.
/// Uses git CLI — works whether the repo is plain git or jj-backed.
pub fn fetch_sentinel_branch(repo_path: &Path) -> Result<()> {
    let out = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or("."),
               "fetch", "origin", SENTINEL_BRANCH, "-q"])
        .output()
        .context("git fetch")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git fetch failed: {}", stderr);
    }
    Ok(())
}

/// Read all sentinels from origin/claude-deploy-sentinels without checking out.
/// Uses gix for fast tree walking.
pub fn read_all(repo_path: &Path) -> Result<Vec<Sentinel>> {
    fetch_sentinel_branch(repo_path)?;

    // Use git show-ref + git cat-file to read without gix complexity for now
    // gix tree walking is verbose; shell is simpler for this read path
    let ls = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or("."),
               "ls-tree", "-r", "--name-only",
               &format!("origin/{}", SENTINEL_BRANCH)])
        .output()
        .context("git ls-tree")?;

    let names: Vec<String> = String::from_utf8_lossy(&ls.stdout)
        .lines()
        .filter(|l| l.starts_with("run-"))
        .map(|l| l.to_string())
        .collect();

    let mut sentinels = Vec::new();
    for name in names {
        let content_out = Command::new("git")
            .args(["-C", repo_path.to_str().unwrap_or("."),
                   "show", &format!("origin/{}:{}", SENTINEL_BRANCH, name)])
            .output()
            .context("git show")?;

        if content_out.status.success() {
            let content = String::from_utf8_lossy(&content_out.stdout);
            sentinels.push(Sentinel::parse(&name, &content));
        }
    }

    sentinels.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sentinels)
}

/// Generate a unique sentinel filename.
/// Format: run-<ref8>-<YYYYMMDDTHHmmss>-<rand4>
pub fn new_name(repo_path: &Path) -> Result<String> {
    let head_out = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or("."),
               "rev-parse", "--short=8", "HEAD"])
        .output()
        .context("git rev-parse HEAD")?;

    let ref8 = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
    let ts = Utc::now().format("%Y%m%dT%H%M%S");
    let rand = rand4();
    Ok(format!("run-{}-{}-{}", ref8, ts, rand))
}

fn rand4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{:04x}", (nanos ^ pid) & 0xffff)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{}:", key)).map(|s| s.trim())
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}
