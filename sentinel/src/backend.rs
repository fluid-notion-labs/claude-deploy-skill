//! VCS backend abstraction.
//!
//! All sentinel git operations go through [`Backend`]. Currently only
//! [`GitShellBackend`] is implemented — it shells out to `git`, which means
//! it works with plain git repos, jj-backed repos (via `jj git export`), and
//! anything else that presents a git remote.
//!
//! Future backends:
//!   - `GixBackend`  — pure Rust gitoxide, no subprocess for read ops
//!   - `JjBackend`   — jj workspaces, no branch checkout thrashing

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sentinel::SENTINEL_BRANCH;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

pub trait Backend: Send + Sync {
    // --- repo info ---

    /// Current branch name (e.g. "main").
    fn current_branch(&self) -> Result<String>;

    /// Short SHA of HEAD (8 chars).
    fn head_short(&self) -> Result<String>;

    /// Full SHA of HEAD.
    fn head_sha(&self) -> Result<String>;

    // --- sentinel branch reads (no checkout) ---

    /// Fetch origin/claude-deploy-sentinels.
    fn fetch_sentinel_branch(&self) -> Result<()>;

    /// List sentinel file names on origin/claude-deploy-sentinels.
    fn list_sentinels(&self) -> Result<Vec<String>>;

    /// Read raw content of a sentinel file from origin/claude-deploy-sentinels.
    fn read_sentinel(&self, name: &str) -> Result<String>;

    // --- sentinel branch writes ---

    /// Ensure the sentinel branch exists locally and on origin (creates orphan if needed).
    fn ensure_sentinel_branch(&self) -> Result<()>;

    /// Write a sentinel file, commit, and push to sentinel branch.
    /// Leaves working tree on whatever branch it was on.
    fn push_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()>;

    /// Update an existing sentinel file, commit, and push.
    /// Precondition: sentinel branch is currently checked out.
    fn update_sentinel_checked_out(&self, name: &str, content: &str, commit_msg: &str) -> Result<()>;

    // --- branch management ---

    /// Checkout a local branch. Fails if working tree is dirty.
    fn checkout(&self, branch: &str) -> Result<()>;

    /// Pull --ff-only from origin for a branch.
    fn pull_ff(&self, branch: &str) -> Result<()>;

    /// Pull --ff-only sentinel branch and return success/failure.
    fn pull_sentinel_ff(&self) -> Result<bool>;

    // --- optimistic claim ---

    /// Try to claim a sentinel: write status=claiming+worker, push.
    /// Returns Ok(true) if we won the race, Ok(false) if another worker got it.
    fn claim_sentinel(&self, name: &str, worker: &str) -> Result<bool>;

    /// Read the worker field of a sentinel from origin (post-claim verification).
    fn sentinel_worker_on_origin(&self, name: &str) -> Result<Option<String>>;

    // --- main branch writes ---

    /// Add paths and commit+push to a branch.
    fn commit_and_push(&self, paths: &[&Path], msg: &str, branch: &str) -> Result<String>;

    /// SHA of a ref on origin.
    fn origin_sha(&self, branch: &str) -> Result<String>;
}

// ---------------------------------------------------------------------------
// GitShellBackend
// ---------------------------------------------------------------------------

/// Backend implementation that shells out to `git`.
/// Works with any repo that has a `git` remote named `origin`.
pub struct GitShellBackend {
    pub repo: PathBuf,
}

impl GitShellBackend {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self { repo: repo.into() }
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .arg("-C").arg(&self.repo)
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

    /// Run git, return true if exit 0.
    fn git_ok(&self, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C").arg(&self.repo)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn sentinel_file_path(&self, name: &str) -> PathBuf {
        self.repo.join(name)
    }

    fn worker_id() -> String {
        let host = std::env::var("HOSTNAME")
            .or_else(|_| {
                Command::new("hostname")
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .map_err(|_| std::env::VarError::NotPresent)
            })
            .unwrap_or_else(|_| "unknown".into());
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
        Ok(())
    }

    fn list_sentinels(&self) -> Result<Vec<String>> {
        let out = self.git(&[
            "ls-tree", "-r", "--name-only",
            &format!("origin/{}", SENTINEL_BRANCH),
        ])?;
        Ok(out.lines()
            .filter(|l| l.starts_with("run-"))
            .map(|l| l.to_string())
            .collect())
    }

    fn read_sentinel(&self, name: &str) -> Result<String> {
        self.git(&["show", &format!("origin/{}:{}", SENTINEL_BRANCH, name)])
    }

    fn ensure_sentinel_branch(&self) -> Result<()> {
        // Fetch to see if origin already has it
        let _ = self.git(&["fetch", "origin", SENTINEL_BRANCH, "-q"]);

        let origin_exists = self.git_ok(&[
            "rev-parse", "--verify",
            &format!("origin/{}", SENTINEL_BRANCH),
        ]);

        if origin_exists {
            // Set up local tracking if missing
            if !self.git_ok(&["rev-parse", "--verify", SENTINEL_BRANCH]) {
                self.git(&["checkout", "--track",
                    &format!("origin/{}", SENTINEL_BRANCH), "-q"])?;
                let orig = self.current_branch()?;
                self.git(&["checkout", &orig, "-q"])?;
            }
            return Ok(());
        }

        // Create orphan
        let orig = self.current_branch()?;
        self.git(&["checkout", "--orphan", SENTINEL_BRANCH, "-q"])?;
        self.git(&["reset", "--hard", "-q"])?;
        self.git(&["commit", "--allow-empty",
            "-m", "init: claude-deploy-sentinels branch", "-q"])?;
        self.git(&["push", "origin", SENTINEL_BRANCH, "-q"])?;
        self.git(&["checkout", &orig, "-q"])?;
        eprintln!("  → created sentinel branch: {}", SENTINEL_BRANCH);
        Ok(())
    }

    fn push_sentinel(&self, name: &str, content: &str, commit_msg: &str) -> Result<()> {
        let orig = self.current_branch()?;

        // Checkout sentinel branch
        self.git(&["checkout", SENTINEL_BRANCH, "-q"])?;
        self.git(&["pull", "--ff-only", "origin", SENTINEL_BRANCH, "-q"])
            .unwrap_or_default();

        // Write file
        std::fs::write(self.sentinel_file_path(name), content)
            .with_context(|| format!("write sentinel {}", name))?;

        self.git(&["add", name])?;
        self.git(&["commit", "-m", commit_msg, "-q"])?;
        self.git(&["push", "origin", SENTINEL_BRANCH, "-q"])?;

        self.git(&["checkout", &orig, "-q"])?;
        Ok(())
    }

    fn update_sentinel_checked_out(&self, name: &str, content: &str, commit_msg: &str) -> Result<()> {
        std::fs::write(self.sentinel_file_path(name), content)
            .with_context(|| format!("write sentinel {}", name))?;
        self.git(&["add", name])?;
        self.git(&["commit", "-m", commit_msg, "-q"])?;
        self.git(&["push", "origin", SENTINEL_BRANCH, "-q"])?;
        Ok(())
    }

    fn checkout(&self, branch: &str) -> Result<()> {
        self.git(&["checkout", branch, "-q"])?;
        Ok(())
    }

    fn pull_ff(&self, branch: &str) -> Result<()> {
        self.git(&["pull", "--ff-only", "origin", branch, "-q"])?;
        Ok(())
    }

    fn pull_sentinel_ff(&self) -> Result<bool> {
        Ok(self.git_ok(&["pull", "--ff-only", "origin", SENTINEL_BRANCH, "-q"]))
    }

    fn claim_sentinel(&self, name: &str, worker: &str) -> Result<bool> {
        // Read current content, patch status+worker, commit+push
        let path = self.sentinel_file_path(name);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read sentinel {}", name))?;

        let patched = set_fields(&content, &[
            ("status", "claiming"),
            ("worker", worker),
        ]);

        std::fs::write(&path, &patched)?;
        self.git(&["add", name])?;
        self.git(&["commit", "-m",
            &format!("sentinel: claiming {} [{}]", name, worker), "-q"])?;

        // Push — non-ff = someone else got there first
        if !self.git_ok(&["push", "origin", SENTINEL_BRANCH, "-q"]) {
            // Reset to origin
            let _ = self.git(&["reset", "--hard",
                &format!("origin/{}", SENTINEL_BRANCH), "-q"]);
            return Ok(false);
        }

        // Re-fetch and verify our worker landed
        let _ = self.git(&["fetch", "origin", SENTINEL_BRANCH, "-q"]);
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

    fn commit_and_push(&self, paths: &[&Path], msg: &str, branch: &str) -> Result<String> {
        for p in paths {
            self.git(&["add", p.to_str().context("non-utf8 path")?])?;
        }
        self.git(&["commit", "-m", msg, "-q"])?;
        self.git(&["push", "origin", branch, "-q"])?;
        Ok(self.head_sha()?)
    }

    fn origin_sha(&self, branch: &str) -> Result<String> {
        Ok(self.git(&["rev-parse", &format!("origin/{}", branch)])?
            .trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// Helpers (used internally, also exported for commands)
// ---------------------------------------------------------------------------

/// Patch a set of key:value fields in sentinel file content.
/// Replaces existing keys in-place; inserts new ones before the first blank line.
pub fn set_fields(content: &str, fields: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    for (key, val) in fields {
        let prefix = format!("{}:", key);
        let new_line = format!("{}: {}", key, val);

        if let Some(i) = lines.iter().position(|l| l.starts_with(&prefix)) {
            lines[i] = new_line;
        } else {
            // Insert before first blank line (header/body separator)
            let insert_at = lines.iter().position(|l| l.trim().is_empty())
                .unwrap_or(lines.len());
            lines.insert(insert_at, new_line);
        }
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') { result.push('\n'); }
    result
}

/// Worker identity string: hostname-pid
pub fn worker_id() -> String {
    GitShellBackend::worker_id()
}
