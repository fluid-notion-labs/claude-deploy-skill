use crate::backend::{worker_id, set_fields, Backend};
use crate::sentinel::{self, Sentinel, Status, SENTINEL_BRANCH};
use anyhow::Result;
use chrono::Utc;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
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

    let mode = if commands_mode { "" } else { " [no-commands]" };
    println!("Watching {} [{}]{} (every {}s) — Ctrl-C to stop",
        repo_path.display(), branch, mode, interval);

    if commands_mode {
        // Sets up .git/claude-sentinel-wt/ — no branch switch in main tree
        backend.ensure_sentinel_branch()?;
    }

    let mut last_ref = backend.head_sha()?;

    loop {
        let ts = Utc::now().format("%H:%M:%S").to_string();

        if commands_mode {
            // Pull main first — sentinels run against latest
            let _ = backend.pull_main(&branch);

            // Fetch + pull into worktree — main tree untouched
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

                // Claim via worktree — no checkout of main tree
                let worker = worker_id();
                match backend.claim_sentinel(&s.name, &worker) {
                    Ok(false) => {
                        eprintln!("  ℹ {} claimed by another worker — skipping", s.name);
                        continue;
                    }
                    Err(e) => {
                        eprintln!("  ⚠ claim failed: {} — skipping", e);
                        continue;
                    }
                    Ok(true) => {}
                }

                // Verify claim on origin
                match backend.sentinel_worker_on_origin(&s.name) {
                    Ok(Some(w)) if w == worker => {}
                    Ok(Some(other)) => {
                        eprintln!("  ℹ claim race lost to {} — skipping", other);
                        continue;
                    }
                    _ => {
                        eprintln!("  ⚠ could not verify claim — skipping");
                        continue;
                    }
                }

                if let Err(e) = run_sentinel(backend, &repo_path, &branch, s) {
                    eprintln!("  ✗ sentinel error: {}", e);
                    let _ = mark_failed(backend, &repo_path, &s.name, &e.to_string());
                }

                println!("[{}] ✓ done", Utc::now().format("%H:%M:%S"));
            }
        }

        // Watch main — no branch switch needed
        let _ = backend.pull_main(&branch);
        let after = backend.head_sha().unwrap_or_default();
        if last_ref != after {
            println!("[{}] ✓ main → {}", ts, &after[..8]);
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
        anyhow::bail!("main_branch is sentinel branch — refusing");
    }

    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    println!("  → {} | ref: {} | capture: {}",
        s.name,
        s.main_ref.as_deref().unwrap_or("-"),
        s.capture.as_deref().unwrap_or("none"));

    // claiming → running (via worktree, no main-tree checkout)
    let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(&s.name);
    let content = std::fs::read_to_string(&wt_path)?;
    let patched = set_fields(&content, &[("status", "running"), ("ran", &ts)]);
    backend.update_sentinel(&s.name, &patched,
        &format!("sentinel: running {}", s.name))?;

    // Build script
    let tmp_script = std::env::temp_dir().join(format!("claude-run-{}.sh", s.name));
    let tmp_log    = std::env::temp_dir().join(format!("claude-log-{}.txt", s.name));
    std::fs::write(&tmp_script, format!(
        "#!/usr/bin/env bash\nset -e\ncd {}\n{}",
        shell_escape(repo_path.to_str().unwrap_or(".")),
        s.script_body
    ))?;

    // Run script — stream output live, tee to log buffer
    let mut child = std::process::Command::new("bash")
        .arg(&tmp_script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut log_lines: Vec<String> = Vec::new();

    // Drain stdout + stderr concurrently using threads
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let tx2 = tx.clone();

    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send((false, line));
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx2.send((true, line));
        }
    });

    for (is_err, line) in rx {
        if is_err {
            eprintln!("  | {}", line);
        } else {
            println!("  | {}", line);
        }
        log_lines.push(line);
    }

    let exit_ok = child.wait()?.success();
    let log_content = log_lines.join("\n") + "\n";
    std::fs::write(&tmp_log, &log_content)?;
    let _ = std::fs::remove_file(&tmp_script);
    println!("  → {}", if exit_ok { "succeeded" } else { "FAILED" });

    // Capture to main if specified
    let mut result_ref = None;
    if let Some(capture) = &s.capture {
        let capture_path = repo_path.join(capture);
        if capture_path.is_dir() {
            let msg = s.msg.as_deref()
                .unwrap_or(&format!("sentinel results: {}", s.name))
                .to_string();
            match backend.commit_and_push(&[&capture_path], &msg, main_branch) {
                Ok(sha) => {
                    println!("  → captured: {} ({})", msg, &sha[..8]);
                    result_ref = Some(sha);
                }
                Err(e) => eprintln!("  ⚠ capture failed: {}", e),
            }
        } else {
            eprintln!("  ⚠ capture dir not found: {}", capture_path.display());
        }
    }

    // Write outcome via worktree — no checkout
    let final_status = if exit_ok { "success" } else { "failure" };
    let completed = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    let content = std::fs::read_to_string(&wt_path)?;
    let mut fields = vec![
        ("status", final_status),
        ("completed", completed.as_str()),
    ];
    let result_str;
    if let Some(ref sha) = result_ref {
        result_str = sha.clone();
        fields.push(("result-ref", result_str.as_str()));
    }
    let mut patched = set_fields(&content, &fields);
    patched.push_str(&format!("\n# --- log ---\n{}",
        log_content.lines()
            .map(|l| format!("# {}\n", l))
            .collect::<String>()));
    let _ = std::fs::remove_file(&tmp_log);

    backend.update_sentinel(&s.name, &patched,
        &format!("sentinel: {} {}", final_status, s.name))?;

    println!("  → sentinel {} ✓", final_status);
    Ok(())
}

fn mark_failed(backend: &dyn Backend, repo_path: &Path, name: &str, error: &str) -> Result<()> {
    let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(name);
    if let Ok(content) = std::fs::read_to_string(&wt_path) {
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut patched = set_fields(&content, &[
            ("status", "failure"),
            ("completed", &ts),
        ]);
        patched.push_str(&format!("\n# --- log ---\n# ERROR: {}\n", error));
        let _ = backend.update_sentinel(name, &patched,
            &format!("sentinel: failure (error) {}", name));
    }
    Ok(())
}

fn reap_abandoned(backend: &dyn Backend, repo_path: &Path, timeout_secs: i64) -> Result<()> {
    let sentinels = match sentinel::read_all(backend) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    for s in sentinels.iter().filter(|s| {
        matches!(s.status, Status::Running | Status::Claiming)
    }) {
        let age = s.age_secs().unwrap_or(0);
        if age <= timeout_secs { continue; }

        let prev_worker = s.worker.as_deref().unwrap_or("unknown");
        eprintln!("  ⚠ abandoning {} ({}s, worker: {})", s.name, age, prev_worker);

        let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(&s.name);
        if let Ok(content) = std::fs::read_to_string(&wt_path) {
            let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let mut patched = set_fields(&content, &[
                ("status", "abandoned"),
                ("abandoned", &ts),
            ]);
            patched.push_str(&format!(
                "\n# --- abandoned ---\n# Stuck {}s. Worker: {}\n",
                age, prev_worker
            ));
            let _ = backend.update_sentinel(&s.name, &patched,
                &format!("sentinel: abandoned {} ({}s)", s.name, age));
        }
    }
    Ok(())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
