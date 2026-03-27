use crate::sentinel::{self, Sentinel, Status, SENTINEL_BRANCH};
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

pub async fn run(repo_path: PathBuf, commands_mode: bool, interval: u64) -> Result<()> {
    let repo = repo_path.clone();

    // Guard: refuse to start on sentinel branch
    let branch = cmd_output(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = branch.trim().to_string();
    if branch == SENTINEL_BRANCH {
        anyhow::bail!("started on sentinel branch — checkout your working branch first");
    }

    let mode = if commands_mode { " [--commands]" } else { "" };
    println!("Watching {} [{}]{} (every {}s) — Ctrl-C to stop",
        repo.display(), branch, mode, interval);

    if commands_mode {
        sentinel_branch_ensure(&repo).await?;
    }

    let mut last_ref = cmd_output(&repo, &["rev-parse", "HEAD"])?.trim().to_string();

    loop {
        let ts = Utc::now().format("%H:%M:%S").to_string();

        if commands_mode {
            // Fetch sentinel branch
            let _ = Command::new("git")
                .args(["-C", repo.to_str().unwrap_or("."),
                       "fetch", "origin", SENTINEL_BRANCH, "-q"])
                .output();

            // Reap abandoned sentinels
            reap_abandoned(&repo, 600).await?;

            // Get all sentinels
            let sentinels = match sentinel::read_all(&repo) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[{}] ⚠ could not read sentinels: {}", ts, e);
                    sleep(Duration::from_secs(interval)).await;
                    continue;
                }
            };

            for s in sentinels.iter().filter(|s| s.status == Status::New) {
                println!("[{}] ⚡ sentinel: {}", ts, s.name);

                // Checkout sentinel branch
                if !git_ok(&repo, &["checkout", SENTINEL_BRANCH, "-q"]) {
                    eprintln!("  ⚠ could not checkout sentinel branch — skipping {}", s.name);
                    git_ok(&repo, &["checkout", &branch, "-q"]);
                    continue;
                }
                if !git_ok(&repo, &["-c", "pull.ff=only", "pull", "origin", SENTINEL_BRANCH, "-q"]) {
                    eprintln!("  ⚠ pull failed — skipping {}", s.name);
                    git_ok(&repo, &["checkout", &branch, "-q"]);
                    continue;
                }

                // Claim
                match claim(&repo, s).await {
                    Ok(true) => {}
                    Ok(false) => {
                        eprintln!("  ℹ {} claimed by another worker", s.name);
                        git_ok(&repo, &["checkout", &branch, "-q"]);
                        continue;
                    }
                    Err(e) => {
                        eprintln!("  ⚠ claim error: {}", e);
                        git_ok(&repo, &["checkout", &branch, "-q"]);
                        continue;
                    }
                }

                run_sentinel(&repo, &branch, s).await?;
                let ts2 = Utc::now().format("%H:%M:%S").to_string();
                println!("[{}] ✓ done", ts2);
            }
        }

        // Pull main
        let _ = Command::new("git")
            .args(["-C", repo.to_str().unwrap_or("."),
                   "-c", "pull.ff=only", "pull", "origin", &branch, "-q"])
            .output();

        let after = cmd_output(&repo, &["rev-parse", "HEAD"])?.trim().to_string();
        if after != last_ref {
            let log = cmd_output(&repo,
                &["log", "--oneline", &format!("{}..{}", last_ref, after)])?;
            println!("[{}] ✓ main:", ts);
            for line in log.trim().lines().take(5) {
                println!("  {}", line);
            }
            last_ref = after;
        }

        sleep(Duration::from_secs(interval)).await;
    }
}

async fn claim(repo: &Path, s: &Sentinel) -> Result<bool> {
    let worker = format!("{}-{}", 
        hostname::get().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|_| "host".into()),
        std::process::id()
    );
    let sentinel_file = repo.join(&s.name);

    set_field(&sentinel_file, "status", "claiming")?;
    set_field(&sentinel_file, "worker", &worker)?;

    git_cmd(repo, &["add", &s.name])?;
    git_cmd(repo, &["commit", "-m",
        &format!("sentinel: claiming {} [{}]", s.name, worker), "-q"])?;

    // Attempt push — fails non-ff if another worker got there first
    if !git_ok(repo, &["push", "origin", SENTINEL_BRANCH, "-q"]) {
        git_ok(repo, &["reset", "--hard", &format!("origin/{}", SENTINEL_BRANCH)]);
        return Ok(false);
    }

    // Verify our worker field is on origin
    let _ = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."),
               "fetch", "origin", SENTINEL_BRANCH, "-q"])
        .output();

    let origin_worker = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."),
               "show", &format!("origin/{}:{}", SENTINEL_BRANCH, s.name)])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find(|l| l.starts_with("worker:"))
                .map(|l| l.trim_start_matches("worker:").trim().to_string())
        });

    if origin_worker.as_deref() != Some(&worker) {
        git_ok(repo, &["-c", "pull.ff=only", "pull", "origin", SENTINEL_BRANCH, "-q"]);
        return Ok(false);
    }

    Ok(true)
}

async fn run_sentinel(repo: &Path, main_branch: &str, s: &Sentinel) -> Result<()> {
    if main_branch == SENTINEL_BRANCH {
        eprintln!("  ✗ BUG: main_branch is sentinel branch — refusing");
        return Ok(());
    }

    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let sentinel_file = repo.join(&s.name);

    // Transition: claiming → running
    set_field(&sentinel_file, "status", "running")?;
    set_field(&sentinel_file, "ran", &ts)?;
    git_cmd(repo, &["add", &s.name])?;
    git_cmd(repo, &["commit", "-m", &format!("sentinel: running {}", s.name), "-q"])?;
    git_cmd(repo, &["push", "origin", SENTINEL_BRANCH, "-q"])?;

    // Write script to tmp
    let tmpscript = tempfile_path("claude-run", ".sh");
    let tmplog    = tempfile_path("claude-log", ".txt");
    let script = format!("#!/usr/bin/env bash\nset -e\ncd {}\n{}\n",
        shell_escape(repo.to_str().unwrap_or(".")),
        s.script_body.trim()
    );
    std::fs::write(&tmpscript, &script)?;

    // Checkout main + pull
    git_ok(repo, &["checkout", main_branch, "-q"]);
    git_ok(repo, &["-c", "pull.ff=only", "pull", "origin", main_branch, "-q"]);

    // Run
    let log_file = std::fs::File::create(&tmplog)?;
    let log_file2 = log_file.try_clone()?;
    let status = Command::new("bash")
        .arg(&tmpscript)
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file2))
        .status();

    let exit_ok = status.map(|s| s.success()).unwrap_or(false);
    let _ = std::fs::remove_file(&tmpscript);
    let log_content = std::fs::read_to_string(&tmplog).unwrap_or_default();
    let _ = std::fs::remove_file(&tmplog);

    // Print log to our stdout too
    for line in log_content.lines() {
        println!("  {}", line);
    }
    println!("  → script {}", if exit_ok { "succeeded" } else { "FAILED" });

    let mut result_ref = None;

    // Capture
    if let Some(capture) = &s.capture {
        let cap_path = repo.join(capture);
        if cap_path.is_dir() {
            git_ok(repo, &["add", capture]);
            let commit_msg = s.msg.as_deref()
                .unwrap_or(&format!("sentinel results: {}", s.name));
            if git_ok(repo, &["commit", "-m", commit_msg, "-q"]) {
                if git_ok(repo, &["push", "origin", main_branch, "-q"]) {
                    result_ref = cmd_output(repo, &["rev-parse", "HEAD"]).ok()
                        .map(|s| s.trim().to_string());
                    println!("  → captured to main: {} ({})",
                        commit_msg,
                        result_ref.as_deref().map(|r| &r[..8]).unwrap_or("?"));
                } else {
                    eprintln!("  ⚠ capture push failed");
                }
            } else {
                eprintln!("  ℹ nothing to capture in {}", capture);
            }
        } else {
            eprintln!("  ⚠ capture dir not found: {}", cap_path.display());
        }
    }

    // Back to sentinel branch
    git_ok(repo, &["checkout", SENTINEL_BRANCH, "-q"]);
    git_ok(repo, &["-c", "pull.ff=only", "pull", "origin", SENTINEL_BRANCH, "-q"]);

    let final_status = if exit_ok { "success" } else { "failure" };
    set_field(&sentinel_file, "status", final_status)?;
    set_field(&sentinel_file, "completed", &Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string())?;
    if let Some(r) = &result_ref {
        set_field(&sentinel_file, "result-ref", r)?;
    }

    // Append log
    let mut log_section = String::from("\n# --- log ---\n");
    for line in log_content.lines() {
        log_section.push_str("# ");
        log_section.push_str(line);
        log_section.push('\n');
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(&sentinel_file)?;
    use std::io::Write;
    f.write_all(log_section.as_bytes())?;

    git_cmd(repo, &["add", &s.name])?;
    git_cmd(repo, &["commit", "-m", &format!("sentinel: {} {}", final_status, s.name), "-q"])?;
    git_cmd(repo, &["push", "origin", SENTINEL_BRANCH, "-q"])?;

    println!("  → sentinel {} ✓", final_status);
    git_ok(repo, &["checkout", main_branch, "-q"]);

    Ok(())
}

async fn reap_abandoned(repo: &Path, timeout_secs: i64) -> Result<()> {
    let sentinels = match sentinel::read_all(repo) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let orig = cmd_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_default();
    let orig = orig.trim();

    for s in sentinels.iter().filter(|s| matches!(s.status, Status::Running | Status::Claiming)) {
        let age = s.age_secs().unwrap_or(0);
        if age > timeout_secs {
            eprintln!("  ⚠ sentinel stuck >{}m [{}]: {} — abandoning",
                timeout_secs / 60,
                s.worker.as_deref().unwrap_or("?"),
                s.name);

            git_ok(repo, &["checkout", SENTINEL_BRANCH, "-q"]);
            if !git_ok(repo, &["-c", "pull.ff=only", "pull", "origin", SENTINEL_BRANCH, "-q"]) {
                git_ok(repo, &["checkout", orig, "-q"]);
                continue;
            }

            let sentinel_file = repo.join(&s.name);
            let _ = set_field(&sentinel_file, "status", "abandoned");
            let _ = set_field(&sentinel_file, "abandoned",
                &Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string());

            let note = format!("\n# --- abandoned ---\n# Stuck {}s. Worker: {}\n",
                age, s.worker.as_deref().unwrap_or("?"));
            let mut f = std::fs::OpenOptions::new().append(true).open(&sentinel_file)
                .unwrap();
            use std::io::Write;
            let _ = f.write_all(note.as_bytes());

            git_ok(repo, &["add", &s.name]);
            git_ok(repo, &["commit", "-m",
                &format!("sentinel: abandoned {} ({}s)", s.name, age), "-q"]);
            // Best-effort push
            if !git_ok(repo, &["push", "origin", SENTINEL_BRANCH, "-q"]) {
                git_ok(repo, &["reset", "--hard",
                    &format!("origin/{}", SENTINEL_BRANCH)]);
            }
            git_ok(repo, &["checkout", orig, "-q"]);
        }
    }
    Ok(())
}

async fn sentinel_branch_ensure(repo: &Path) -> Result<()> {
    let _ = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."),
               "fetch", "origin", SENTINEL_BRANCH, "-q"])
        .output();

    let remote_exists = Command::new("git")
        .args(["-C", repo.to_str().unwrap_or("."),
               "rev-parse", "--verify", &format!("origin/{}", SENTINEL_BRANCH)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if remote_exists {
        let local_exists = Command::new("git")
            .args(["-C", repo.to_str().unwrap_or("."),
                   "rev-parse", "--verify", SENTINEL_BRANCH])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !local_exists {
            git_ok(repo, &["checkout", "--track",
                &format!("origin/{}", SENTINEL_BRANCH), "-q"]);
            git_ok(repo, &["checkout", "-", "-q"]);
        }
        return Ok(());
    }

    let orig = cmd_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    git_ok(repo, &["checkout", "--orphan", SENTINEL_BRANCH, "-q"]);
    git_ok(repo, &["reset", "--hard", "-q"]);
    git_ok(repo, &["commit", "--allow-empty",
        "-m", "init: claude-deploy-sentinels branch", "-q"]);
    git_ok(repo, &["push", "origin", SENTINEL_BRANCH, "-q"]);
    git_ok(repo, &["checkout", orig.trim(), "-q"]);
    eprintln!("  → created sentinel branch: {}", SENTINEL_BRANCH);
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn git_cmd(repo: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C").arg(repo).args(args)
        .status().context("git")?;
    if !status.success() {
        anyhow::bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C").arg(repo).args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cmd_output(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C").arg(repo).args(args)
        .output().context("git")?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn set_field(path: &Path, key: &str, val: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let new_line = format!("{}: {}", key, val);
    let new_content = if content.lines().any(|l| l.starts_with(&format!("{}:", key))) {
        content.lines()
            .map(|l| if l.starts_with(&format!("{}:", key)) { new_line.as_str() } else { l })
            .collect::<Vec<_>>()
            .join("\n") + "\n"
    } else {
        // Insert before first blank line
        let mut lines: Vec<&str> = content.lines().collect();
        let insert_at = lines.iter().position(|l| l.is_empty()).unwrap_or(lines.len());
        lines.insert(insert_at, &new_line);
        lines.join("\n") + "\n"
    };
    std::fs::write(path, new_content)?;
    Ok(())
}

fn tempfile_path(prefix: &str, suffix: &str) -> PathBuf {
    let ts = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    PathBuf::from(format!("/tmp/{}-{}{}", prefix, ts, suffix))
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
