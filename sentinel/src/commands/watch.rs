use crate::backend::{worker_id, set_fields, Backend};
use crate::sentinel::{self, Sentinel, Status, SENTINEL_BRANCH};
use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

pub async fn run(
    backend: &dyn Backend,
    repo_path: PathBuf,
    commands_mode: bool,
    interval: u64,
) -> Result<()> {
    let branch = backend.current_branch()?;
    if branch == SENTINEL_BRANCH {
        anyhow::bail!("started on sentinel branch — checkout your working branch first");
    }

    let mode = if commands_mode { " [--commands]" } else { "" };
    println!("Watching {} [{}]{} (every {}s) — Ctrl-C to stop",
        repo_path.display(), branch, mode, interval);

    if commands_mode {
        backend.ensure_sentinel_branch()?;
    }

    let mut last_ref = backend.head_sha()?;

    loop {
        let ts = Utc::now().format("%H:%M:%S").to_string();

        if commands_mode {
            let _ = backend.fetch_sentinel_branch();

            reap_abandoned(backend, &repo_path, 600)?;

            let sentinels = match sentinel::read_all(backend) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[{}] ⚠ could not read sentinels: {}", ts, e);
                    sleep(Duration::from_secs(interval)).await;
                    continue;
                }
            };

            for s in sentinels.iter().filter(|s| s.status == Status::New) {
                println!("[{}] ⚡ sentinel: {}", ts, s.name);

                // Checkout sentinel branch + pull
                if let Err(e) = backend.checkout(SENTINEL_BRANCH) {
                    eprintln!("  ⚠ could not checkout sentinel branch: {} — skipping", e);
                    let _ = backend.checkout(&branch);
                    continue;
                }
                if !backend.pull_sentinel_ff()? {
                    eprintln!("  ⚠ pull sentinel branch failed — skipping {}", s.name);
                    let _ = backend.checkout(&branch);
                    continue;
                }

                // Optimistic claim
                let worker = worker_id();
                match backend.claim_sentinel(&s.name, &worker) {
                    Ok(false) => {
                        eprintln!("  ℹ {} claimed by another worker — skipping", s.name);
                        let _ = backend.checkout(&branch);
                        continue;
                    }
                    Err(e) => {
                        eprintln!("  ⚠ claim failed: {} — skipping", e);
                        let _ = backend.checkout(&branch);
                        continue;
                    }
                    Ok(true) => {}
                }

                // Verify our worker landed on origin
                match backend.sentinel_worker_on_origin(&s.name) {
                    Ok(Some(w)) if w == worker => {}
                    Ok(Some(other)) => {
                        eprintln!("  ℹ claim race lost to {} — skipping", other);
                        let _ = backend.pull_sentinel_ff();
                        let _ = backend.checkout(&branch);
                        continue;
                    }
                    _ => {
                        eprintln!("  ⚠ could not verify claim — skipping");
                        let _ = backend.checkout(&branch);
                        continue;
                    }
                }

                if let Err(e) = run_sentinel(backend, &repo_path, &branch, s) {
                    eprintln!("  ✗ sentinel error: {}", e);
                    // Best-effort: mark failure
                    let _ = backend.checkout(SENTINEL_BRANCH);
                    let _ = mark_sentinel_failed(backend, &repo_path, &s.name, &e.to_string());
                    let _ = backend.checkout(&branch);
                }

                println!("[{}] ✓ done", Utc::now().format("%H:%M:%S"));
            }
        }

        // Watch main branch
        let _ = backend.pull_ff(&branch);
        let after = backend.head_sha().unwrap_or_default();
        if last_ref != after {
            println!("[{}] ✓ main advanced to {}", ts, &after[..8]);
            last_ref = after;
        }

        sleep(Duration::from_secs(interval)).await;
    }
}

fn run_sentinel(
    backend: &dyn Backend,
    repo_path: &Path,
    main_branch: &str,
    s: &Sentinel,
) -> Result<()> {
    if main_branch == SENTINEL_BRANCH {
        anyhow::bail!("main_branch is sentinel branch — refusing to run");
    }

    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    println!("  → running: {}  ref: {}  capture: {}",
        s.name,
        s.main_ref.as_deref().unwrap_or("-"),
        s.capture.as_deref().unwrap_or("(none)"));

    // Transition: claiming → running
    let sentinel_path = repo_path.join(&s.name);
    let content = std::fs::read_to_string(&sentinel_path)?;
    let patched = set_fields(&content, &[("status", "running"), ("ran", &ts)]);
    backend.update_sentinel_checked_out(&s.name, &patched,
        &format!("sentinel: running {}", s.name))?;

    // Write temp script
    let tmp_script = std::env::temp_dir().join(format!("claude-run-{}.sh", s.name));
    let tmp_log    = std::env::temp_dir().join(format!("claude-log-{}.txt", s.name));
    let script = format!("#!/usr/bin/env bash\nset -e\ncd {}\n{}",
        shell_escape(repo_path.to_str().unwrap_or(".")),
        s.script_body);
    std::fs::write(&tmp_script, &script)?;

    // Checkout main + pull
    backend.checkout(main_branch)?;
    let _ = backend.pull_ff(main_branch);

    // Run
    let output = std::process::Command::new("bash")
        .arg(&tmp_script)
        .output()?;
    let log_content = format!("{}{}", 
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr));
    std::fs::write(&tmp_log, &log_content)?;
    let _ = std::fs::remove_file(&tmp_script);

    let exit_ok = output.status.success();
    if exit_ok { println!("  → script succeeded"); }
    else        { println!("  → script FAILED"); }

    // Capture if specified
    let mut result_ref = None;
    if let Some(capture) = &s.capture {
        let capture_path = repo_path.join(capture);
        if capture_path.is_dir() {
            let msg = s.msg.as_deref()
                .unwrap_or(&format!("sentinel results: {}", s.name))
                .to_string();
            match backend.commit_and_push(&[&capture_path], &msg, main_branch) {
                Ok(sha) => {
                    result_ref = Some(sha.clone());
                    println!("  → captured to main: {} ({})", msg, &sha[..8]);
                }
                Err(e) => eprintln!("  ⚠ capture push failed: {}", e),
            }
        } else {
            eprintln!("  ⚠ capture dir not found: {}", capture_path.display());
        }
    }

    // Back to sentinel branch to write outcome
    backend.checkout(SENTINEL_BRANCH)?;
    let _ = backend.pull_sentinel_ff();

    let final_status = if exit_ok { "success" } else { "failure" };
    let completed = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    let content = std::fs::read_to_string(&sentinel_path)?;
    let mut fields = vec![
        ("status", final_status),
        ("completed", completed.as_str()),
    ];
    let result_str;
    if let Some(ref sha) = result_ref {
        result_str = sha.clone();
        fields.push(("result-ref", &result_str));
    }
    let mut patched = set_fields(&content, &fields);

    // Append log
    let log_section = format!("\n# --- log ---\n{}",
        log_content.lines()
            .map(|l| format!("# {}", l))
            .collect::<Vec<_>>()
            .join("\n"));
    patched.push_str(&log_section);
    let _ = std::fs::remove_file(&tmp_log);

    backend.update_sentinel_checked_out(&s.name, &patched,
        &format!("sentinel: {} {}", final_status, s.name))?;

    println!("  → sentinel {} ✓", final_status);
    backend.checkout(main_branch)?;
    Ok(())
}

fn mark_sentinel_failed(
    backend: &dyn Backend,
    repo_path: &Path,
    name: &str,
    error: &str,
) -> Result<()> {
    let path = repo_path.join(name);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut patched = set_fields(&content, &[
            ("status", "failure"),
            ("completed", &ts),
        ]);
        patched.push_str(&format!("\n# --- log ---\n# ERROR: {}\n", error));
        let _ = backend.update_sentinel_checked_out(name, &patched,
            &format!("sentinel: failure (error) {}", name));
    }
    Ok(())
}

fn reap_abandoned(backend: &dyn Backend, repo_path: &Path, timeout_secs: i64) -> Result<()> {
    let now = Utc::now();
    let sentinels = match sentinel::read_all(backend) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let orig = backend.current_branch()?;

    for s in sentinels.iter().filter(|s| {
        matches!(s.status, Status::Running | Status::Claiming)
    }) {
        let age = s.age_secs().unwrap_or(0);
        if age <= timeout_secs { continue; }

        let prev_worker = s.worker.as_deref().unwrap_or("unknown");
        eprintln!("  ⚠ abandoned sentinel ({}s, worker: {}): {}", age, prev_worker, s.name);

        backend.checkout(SENTINEL_BRANCH)?;
        if !backend.pull_sentinel_ff()? {
            let _ = backend.checkout(&orig);
            continue;
        }

        let sentinel_path = repo_path.join(&s.name);
        let content = match std::fs::read_to_string(&sentinel_path) {
            Ok(c) => c,
            Err(_) => { let _ = backend.checkout(&orig); continue; }
        };
        let ts = now.format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut patched = set_fields(&content, &[
            ("status", "abandoned"),
            ("abandoned", &ts),
        ]);
        patched.push_str(&format!(
            "\n# --- abandoned ---\n# Stuck {}s. Previous worker: {}\n",
            age, prev_worker
        ));

        let _ = backend.update_sentinel_checked_out(&s.name, &patched,
            &format!("sentinel: abandoned {} (stuck {}s)", s.name, age));

        let _ = backend.checkout(&orig);
    }
    Ok(())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
