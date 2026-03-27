use crate::sentinel::{self, SENTINEL_BRANCH};
use anyhow::{Context, Result};
use chrono::Utc;
use std::io::Read;
use std::path::Path;
use std::process::Command;

pub fn run(
    repo_path: impl AsRef<Path>,
    script: std::path::PathBuf,
    capture: Option<String>,
    msg: Option<String>,
) -> Result<()> {
    let repo_path = repo_path.as_ref();

    // Read script body
    let script_body = if script.to_str() == Some("-") {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(&script)
            .with_context(|| format!("read script: {}", script.display()))?
    };

    // Get current HEAD ref
    let head = cmd_output(repo_path, &["rev-parse", "--short=8", "HEAD"])?;
    let main_ref = head.trim().to_string();
    let created = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let name = sentinel::new_name(repo_path)?;

    // Build sentinel file content
    let mut content = format!(
        "status: new\nmain-ref: {}\ncreated: {}\n",
        main_ref, created
    );
    if let Some(c) = &capture { content.push_str(&format!("capture: {}\n", c)); }
    if let Some(m) = &msg     { content.push_str(&format!("msg: {}\n", m)); }
    content.push('\n');
    content.push_str(script_body.trim_end());
    content.push('\n');

    // Ensure sentinel branch exists and switch to it
    sentinel_branch_ensure(repo_path)?;

    let orig_branch = cmd_output(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let orig_branch = orig_branch.trim();

    git(repo_path, &["checkout", SENTINEL_BRANCH, "-q"])?;
    git(repo_path, &["-c", "pull.ff=only", "pull", "origin", SENTINEL_BRANCH, "-q"])?;

    let sentinel_path = repo_path.join(&name);
    std::fs::write(&sentinel_path, &content)
        .with_context(|| format!("write sentinel: {}", sentinel_path.display()))?;

    git(repo_path, &["add", &name])?;
    git(repo_path, &["commit", "-m", &format!("sentinel: new {}", name), "-q"])?;
    git(repo_path, &["push", "origin", SENTINEL_BRANCH, "-q"])?;

    git(repo_path, &["checkout", orig_branch, "-q"])?;

    println!("✓ pushed sentinel: {}", name);
    if let Some(m) = &msg     { println!("  msg:     {}", m); }
    if let Some(c) = &capture { println!("  capture: {}", c); }

    Ok(())
}

fn sentinel_branch_ensure(repo_path: &Path) -> Result<()> {
    // Fetch — branch may already exist on origin
    let _ = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or("."),
               "fetch", "origin", SENTINEL_BRANCH, "-q"])
        .output();

    // Check if remote branch exists
    let remote_exists = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or("."),
               "rev-parse", "--verify", &format!("origin/{}", SENTINEL_BRANCH)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if remote_exists {
        // Set up local tracking branch if missing
        let local_exists = Command::new("git")
            .args(["-C", repo_path.to_str().unwrap_or("."),
                   "rev-parse", "--verify", SENTINEL_BRANCH])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !local_exists {
            git(repo_path, &["checkout", "--track", &format!("origin/{}", SENTINEL_BRANCH), "-q"])?;
            git(repo_path, &["checkout", "-", "-q"])?;
        }
        return Ok(());
    }

    // Create orphan
    let orig = cmd_output(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    git(repo_path, &["checkout", "--orphan", SENTINEL_BRANCH, "-q"])?;
    git(repo_path, &["reset", "--hard", "-q"])?;
    git(repo_path, &["commit", "--allow-empty", "-m", "init: claude-deploy-sentinels branch", "-q"])?;
    git(repo_path, &["push", "origin", SENTINEL_BRANCH, "-q"])?;
    git(repo_path, &["checkout", orig.trim(), "-q"])?;
    eprintln!("  → created sentinel branch: {}", SENTINEL_BRANCH);
    Ok(())
}

fn git(repo_path: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C").arg(repo_path)
        .args(args)
        .status()
        .context("git")?;
    if !status.success() {
        anyhow::bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn cmd_output(repo_path: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C").arg(repo_path)
        .args(args)
        .output()
        .context("git")?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
