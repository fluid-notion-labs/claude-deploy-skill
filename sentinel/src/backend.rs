//! VCS backend abstraction.
//!
//! All sentinel git operations go through [`Backend`]. Currently only
//! [`GitShellBackend`] is implemented.
//!
//! `GitShellBackend` uses a **git worktree** for the sentinel branch,
//! kept at `.git/claude-sentinel-wt/` inside the repo. This means the
//! main working tree never changes branch — no checkout thrashing, safe
//! under Ctrl-C, invisible to normal git status.
//!
//! Future backends:
//!   - `GixBackend`  — pure Rust gitoxide
//!   - `JjBackend`   — jj workspaces (same idea, native)

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sentinel::SENTINEL_BRANCH;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

pub trait Backend: Send + Sync {
    // --- repo info ---
    fn current_branch(&self) -> Result<String>;
    fn head_short(&self) -> Result<String>;
    fn head_sha(&self) -> Result<String>;

    // --- sentinel branch reads (no checkout, reads from worktree or origin) ---
    fn fetch_sentinel_branch(&self) -> Result<()>;
    fn list_sentinels(&self) -> Result<Vec<String>>;
    fn read_sentinel(&self, name: &str) -> Result<String>;

    // --- sentinel branch writes (all via worktree — no main-tree checkout) ---
    fn ensure_sentinel_branch(&self) -> Result<()>;
    fn push_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()>;
    fn update_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()>;

    // --- optimistic claim ---
    fn claim_sentinel(&self, name: &str, worker: &str) -> Result<bool>;
    fn sentinel_worker_on_origin(&self, name: &str) -> Result<Option<String>>;

    // --- main branch writes ---
    fn pull_main(&self, branch: &str) -> Result<()>;
    fn commit_and_push(&self, paths: &[&Path], msg: &str, branch: &str) -> Result<String>;
}

// ---------------------------------------------------------------------------
// GitShellBackend
// ---------------------------------------------------------------------------

pub struct GitShellBackend {
    /// Main working tree — stays on main branch always.
    pub repo: PathBuf,
    /// Sentinel worktree — `.git/claude-sentinel-wt/`, always on sentinel branch.
    sentinel_wt: PathBuf,
}

impl GitShellBackend {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        let repo = repo.into();
        let sentinel_wt = repo.join(".git").join("claude-sentinel-wt");
        Self { repo, sentinel_wt }
    }

    /// Run git in the main repo.
    fn git(&self, args: &[&str]) -> Result<String> {
        self.git_in(&self.repo, args)
    }

    /// Run git in the sentinel worktree.
    fn git_wt(&self, args: &[&str]) -> Result<String> {
        self.git_in(&self.sentinel_wt, args)
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .arg("-C").arg(dir)
            .args(args)
            .output()
            .with_context(|| format!("git {}", args.join(" ")))?;

        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn git_ok(&self, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C").arg(&self.repo)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn git_wt_ok(&self, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C").arg(&self.sentinel_wt)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Path to a sentinel file in the worktree.
    pub fn sentinel_path(&self, name: &str) -> PathBuf {
        self.sentinel_wt.join(name)
    }

    /// Ensure the sentinel worktree exists and is up to date.
    /// Called lazily — safe to call multiple times.
    fn ensure_worktree(&self) -> Result<()> {
        if self.sentinel_wt.join("HEAD").exists() {
            // Already set up — check we are on the branch (not detached), then pull
            let head = self.git_wt(&["symbolic-ref", "--short", "HEAD"])
                .unwrap_or_default();
            if head.trim() != SENTINEL_BRANCH {
                // Detached — remove and recreate
                eprintln!("  → worktree in detached state, recreating...");
                let _ = self.git(&["worktree", "remove", "--force",
                    self.sentinel_wt.to_str().unwrap_or(".")]);
            } else {
                let _ = self.git_wt(&["pull", "--ff-only", "origin", SENTINEL_BRANCH, "-q"]);
                return Ok(());
            }
        }

        // Fetch sentinel branch from origin
        let _ = self.git(&["fetch", "origin", SENTINEL_BRANCH, "-q"]);

        let origin_exists = self.git_ok(&[
            "rev-parse", "--verify",
            &format!("origin/{}", SENTINEL_BRANCH),
        ]);

        if !origin_exists {
            // Create orphan sentinel branch first (push to origin)
            // Use a temp worktree approach to avoid touching main tree
            let tmp = self.repo.join(".git").join("claude-sentinel-tmp");
            let _ = std::fs::remove_dir_all(&tmp);

            self.git(&["worktree", "add", "--orphan",
                "-b", SENTINEL_BRANCH,
                tmp.to_str().context("non-utf8 path")?, "-q"])?;

            // Empty commit to initialise
            self.git_in(&tmp, &["commit", "--allow-empty",
                "-m", "init: claude-deploy-sentinels branch", "-q"])?;
            self.git_in(&tmp, &["push", "origin", SENTINEL_BRANCH, "-q"])?;

            // Remove temp, add permanent worktree
            self.git(&["worktree", "remove", "--force",
                tmp.to_str().context("non-utf8 path")?])?;
            eprintln!("  → created sentinel branch: {}", SENTINEL_BRANCH);
        }

        // Add permanent worktree with local tracking branch.
        // Use -B to reset if branch already exists locally.
        self.git(&[
            "worktree", "add",
            "-B", SENTINEL_BRANCH,
            self.sentinel_wt.to_str().context("non-utf8 path")?,
            &format!("origin/{}", SENTINEL_BRANCH),
            "-q",
        ])?;

        eprintln!("  → sentinel worktree: {}", self.sentinel_wt.display());
        Ok(())
    }

    pub fn worker_id() -> String {
        let host = std::env::var("HOSTNAME")
            .unwrap_or_else(|_| {
                Command::new("hostname")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|_| "unknown".into())
            });
        format!("{}-{}", host, std::process::id())
    }
}

impl Backend for GitShellBackend {
    fn current_branch(&self) -> Result<String> {
        Ok(self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string())
    }

    fn head_short(&self) -> Result<String> {
        Ok(self.git(&["rev-parse", "--short=8", "HEAD"])?.trim().to_string())
    }

    fn head_sha(&self) -> Result<String> {
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_string())
    }

    fn fetch_sentinel_branch(&self) -> Result<()> {
        self.git(&["fetch", "origin", SENTINEL_BRANCH, "-q"])?;
        // Pull in worktree if it exists
        if self.sentinel_wt.join("HEAD").exists() {
            let _ = self.git_wt(&["pull", "--ff-only", "origin", SENTINEL_BRANCH, "-q"]);
        }
        Ok(())
    }

    fn list_sentinels(&self) -> Result<Vec<String>> {
        // Always read from origin ref — guaranteed fresh after fetch_sentinel_branch.
        // Worktree is for writes only.
        let out = self.git(&["ls-tree", "-r", "--name-only",
            &format!("origin/{}", SENTINEL_BRANCH)])?;
        Ok(out.lines()
            .filter(|l| l.starts_with("run-"))
            .map(|l| l.to_string())
            .collect())
    }

    fn read_sentinel(&self, name: &str) -> Result<String> {
        // Always read from origin ref — never from stale worktree disk state.
        self.git(&["show", &format!("origin/{}:{}", SENTINEL_BRANCH, name)])
    }

    fn ensure_sentinel_branch(&self) -> Result<()> {
        self.ensure_worktree()
    }

    fn push_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()> {
        self.ensure_worktree()?;
        std::fs::write(self.sentinel_path(name), content)
            .with_context(|| format!("write sentinel {}", name))?;
        self.git_wt(&["add", name])?;
        self.git_wt(&["commit", "-m", commit_msg, "-q"])?;
        self.git_wt(&["push", "origin", SENTINEL_BRANCH, "-q"])?;
        Ok(())
    }

    fn update_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()> {
        // Worktree must already be set up
        std::fs::write(self.sentinel_path(name), content)
            .with_context(|| format!("write sentinel {}", name))?;
        self.git_wt(&["add", name])?;
        self.git_wt(&["commit", "-m", commit_msg, "-q"])?;
        self.git_wt(&["push", "origin", SENTINEL_BRANCH, "-q"])?;
        Ok(())
    }

    fn claim_sentinel(&self, name: &str, worker: &str) -> Result<bool> {
        // Pull worktree to latest before claiming — ensures file is on disk and current
        let _ = self.git_wt(&["pull", "--ff-only", "origin", SENTINEL_BRANCH, "-q"]);

        let path = self.sentinel_path(name);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read sentinel {}", name))?;

        // Re-check status from disk — bail if already claimed/running
        if let Some(status) = content.lines()
            .find(|l| l.starts_with("status:"))
            .and_then(|l| l.strip_prefix("status:"))
            .map(|s| s.trim())
        {
            if status != "new" {
                return Ok(false);
            }
        }

        let patched = set_fields(&content, &[
            ("status", "claiming"),
            ("worker", worker),
        ]);
        std::fs::write(&path, &patched)?;

        self.git_wt(&["add", name])?;
        self.git_wt(&["commit", "-m",
            &format!("sentinel: claiming {} [{}]", name, worker), "-q"])?;

        // Push — non-ff means another worker got there first
        if !self.git_wt_ok(&["push", "origin", SENTINEL_BRANCH, "-q"]) {
            let _ = self.git_wt(&["reset", "--hard",
                &format!("origin/{}", SENTINEL_BRANCH), "-q"]);
            return Ok(false);
        }

        // Re-fetch to verify our commit landed
        let _ = self.git_wt(&["fetch", "origin", SENTINEL_BRANCH, "-q"]);
        Ok(true)
    }

    fn sentinel_worker_on_origin(&self, name: &str) -> Result<Option<String>> {
        let content = self.git(&[
            "show", &format!("origin/{}:{}", SENTINEL_BRANCH, name),
        ])?;
        Ok(content.lines()
            .find(|l| l.starts_with("worker:"))
            .and_then(|l| l.strip_prefix("worker:"))
            .map(|s| s.trim().to_string()))
    }

    fn pull_main(&self, branch: &str) -> Result<()> {
        self.git(&["pull", "--ff-only", "origin", branch, "-q"])?;
        Ok(())
    }

    fn commit_and_push(&self, paths: &[&Path], msg: &str, branch: &str) -> Result<String> {
        for p in paths {
            self.git(&["add", p.to_str().context("non-utf8 path")?])?;
        }
        self.git(&["commit", "-m", msg, "-q"])?;
        self.git(&["push", "origin", branch, "-q"])?;
        Ok(self.head_sha()?)
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used by commands)
// ---------------------------------------------------------------------------

/// Patch key:value fields in sentinel file content.
/// Replaces existing keys in-place; inserts new ones before the first blank line.
pub fn set_fields(content: &str, fields: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    for (key, val) in fields {
        let prefix = format!("{}:", key);
        let new_line = format!("{}: {}", key, val);

        if let Some(i) = lines.iter().position(|l| l.starts_with(&prefix)) {
            lines[i] = new_line;
        } else {
            let insert_at = lines.iter().position(|l| l.trim().is_empty())
                .unwrap_or(lines.len());
            lines.insert(insert_at, new_line);
        }
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') { result.push('\n'); }
    result
}

pub fn worker_id() -> String {
    GitShellBackend::worker_id()
}
