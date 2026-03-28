use crate::backend::{worker_id, set_fields, Backend};
use crate::sentinel::{self, Sentinel, Status, TokenFile, SENTINEL_BRANCH};
use anyhow::Result;
use chrono::Utc;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Produce queue — serializes all git pushes on a background thread
// ---------------------------------------------------------------------------

enum PushJob {
    /// Write + push a sentinel file update (status change, log, etc.)
    UpdateSentinel {
        name: String,
        content: String,
        commit_msg: String,
    },
    /// Commit + push capture dir to main branch
    CommitCapture {
        paths: Vec<PathBuf>,
        msg: String,
        branch: String,
        /// Channel to send back the resulting SHA (or error)
        reply: mpsc::SyncSender<Result<String>>,
    },
}

/// Spawn a background thread that serializes all git push operations.
/// Returns a sender; drop it to shut the thread down.
fn spawn_push_thread(backend: Arc<dyn Backend>) -> mpsc::Sender<PushJob> {
    let (tx, rx) = mpsc::channel::<PushJob>();
    std::thread::spawn(move || {
        for job in rx {
            match job {
                PushJob::UpdateSentinel { name, content, commit_msg } => {
                    if let Err(e) = backend.update_sentinel(&name, &content, &commit_msg) {
                        eprintln!("  ⚠ push thread: update_sentinel {}: {}", name, e);
                    }
                }
                PushJob::CommitCapture { paths, msg, branch, reply } => {
                    let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    let result = backend.commit_and_push(&path_refs, &msg, &branch);
                    let _ = reply.send(result);
                }
            }
        }
    });
    tx
}

// ---------------------------------------------------------------------------
// Watch loop
// ---------------------------------------------------------------------------

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
        backend.ensure_sentinel_branch()?;
    }

    // Push thread gets its own backend instance (same repo path)
    let push_backend = Arc::new(crate::backend::GitShellBackend::new(repo_path.clone()));
    let push_tx = spawn_push_thread(push_backend);

    let mut last_ref = backend.head_sha()?;

    // Token refresh state
    let mut token_expiry: Option<chrono::DateTime<Utc>> = None;
    let mut token_org: Option<String> = None;
    const REFRESH_SECS_BEFORE: i64 = 7 * 60; // refresh 7 min before expiry

    loop {
        let ts = Utc::now().format("%H:%M:%S").to_string();

        // --- Token refresh check (commands mode only — needs sentinel branch) ---
        if commands_mode {
            let now = Utc::now();
            let needs_refresh = token_expiry
                .map(|exp| (exp - now).num_seconds() < REFRESH_SECS_BEFORE)
                .unwrap_or(false);

            if needs_refresh {
                // First check if a fresh tok- file already landed (e.g. from another session)
                let fresh = sentinel::read_tokens(backend)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|t| t.is_valid() && t.name > token_org.as_deref().unwrap_or(""));

                if let Some(tok) = fresh {
                    let mins = (tok.expires - now).num_minutes();
                    println!("[{}] 🔑 token refreshed from branch (org: {}, {}min remaining)",
                        ts, tok.org, mins);
                    update_remote_token(&repo_path, &tok.token);
                    token_expiry = Some(tok.expires);
                    token_org = Some(tok.name.clone());
                } else {
                    // Generate fresh token ourselves
                    let org = token_org.as_deref().unwrap_or("default");
                    // Extract just the org name from "tok-<org>-<ts>" if needed
                    let org_name = org.strip_prefix("tok-")
                        .and_then(|s| s.rsplitn(2, '-').nth(1))
                        .unwrap_or(org);
                    println!("[{}] 🔄 token expiring soon — generating fresh token (org: {})", ts, org_name);
                    match crate::github_token::AppConfig::load(org_name)
                        .and_then(|cfg| crate::github_token::generate_token(&cfg))
                    {
                        Ok(gen) => {
                            let tok_name = crate::sentinel::TokenFile::file_name(&gen.org);
                            let content = gen.to_tok_file();
                            match backend.push_token_file(&tok_name, &content) {
                                Ok(_) => {
                                    let mins = (gen.expires - now).num_minutes();
                                    println!("[{}] 🔑 fresh token pushed (org: {}, {}min remaining)",
                                        ts, gen.org, mins);
                                    update_remote_token(&repo_path, &gen.token);
                                    token_expiry = Some(gen.expires);
                                    token_org = Some(tok_name);
                                }
                                Err(e) => eprintln!("[{}] ⚠ push token file failed: {}", ts, e),
                            }
                        }
                        Err(e) => eprintln!("[{}] ⚠ token refresh failed: {}", ts, e),
                    }
                }
            }

            // Pick up initial token from branch on first loop / if not yet set
            if token_expiry.is_none() {
                if let Ok(tokens) = sentinel::read_tokens(backend) {
                    if let Some(tok) = tokens.into_iter().find(|t| t.is_valid()) {
                        let mins = (tok.expires - Utc::now()).num_minutes();
                        println!("[{}] 🔑 token loaded (org: {}, {}min remaining)",
                            ts, tok.org, mins);
                        update_remote_token(&repo_path, &tok.token);
                        token_expiry = Some(tok.expires);
                        token_org = Some(tok.name.clone());
                    }
                }
            }
        }

        // Pull main once at top of loop
        let _ = backend.pull_main(&branch);
        let after = backend.head_sha().unwrap_or_default();
        if last_ref != after {
            println!("[{}] ✓ main → {}", ts, &after[..8]);
            last_ref = after.clone();
        }

        if commands_mode {
            let _ = backend.fetch_sentinel_branch();

            reap_abandoned(backend, &repo_path, &push_tx, 600)?;

            let sentinels = match sentinel::read_all(backend) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[{}] ⚠ could not read sentinels: {}", ts, e);
                    sleep(Duration::from_secs(interval)).await;
                    continue;
                }
            };

            // Consume queue — claim + run scripts, result pushes go to produce queue
            for s in sentinels.iter().filter(|s| s.status == Status::New) {
                let ts = Utc::now().format("%H:%M:%S").to_string();
                println!("[{}] ⚡ sentinel: {}", ts, s.name);

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

                if let Err(e) = run_sentinel(&repo_path, &branch, s, &push_tx) {
                    eprintln!("  ✗ sentinel error: {}", e);
                    mark_failed(&repo_path, &s.name, &e.to_string(), &push_tx);
                }

                println!("[{}] ✓ done", Utc::now().format("%H:%M:%S"));
            }
        }

        sleep(Duration::from_secs(interval)).await;
    }
}

// ---------------------------------------------------------------------------
// Run a single sentinel — script execution, result pushed via produce queue
// ---------------------------------------------------------------------------

fn run_sentinel(
    repo_path: &Path,
    main_branch: &str,
    s: &Sentinel,
    push_tx: &mpsc::Sender<PushJob>,
) -> Result<()> {
    if main_branch == SENTINEL_BRANCH {
        anyhow::bail!("main_branch is sentinel branch — refusing");
    }

    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    println!("  → {} | ref: {} | capture: {}",
        s.name,
        s.main_ref.as_deref().unwrap_or("-"),
        s.capture.as_deref().unwrap_or("none"));

    // Write running status to disk, queue the push
    let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(&s.name);
    let content = std::fs::read_to_string(&wt_path)?;
    let running_content = set_fields(&content, &[("status", "running"), ("ran", &ts)]);
    std::fs::write(&wt_path, &running_content)?;
    push_tx.send(PushJob::UpdateSentinel {
        name: s.name.clone(),
        content: running_content,
        commit_msg: format!("sentinel: running {}", s.name),
    }).ok();

    // Build + run script
    let tmp_script = std::env::temp_dir().join(format!("claude-run-{}.sh", s.name));
    std::fs::write(&tmp_script, format!(
        "#!/usr/bin/env bash\nset -e\ncd {}\n{}",
        shell_escape(repo_path.to_str().unwrap_or(".")),
        s.script_body
    ))?;

    let mut child = std::process::Command::new("bash")
        .arg(&tmp_script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut log_lines: Vec<String> = Vec::new();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (line_tx, line_rx) = std::sync::mpsc::channel::<(bool, String)>();
    let line_tx2 = line_tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = line_tx.send((false, line));
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = line_tx2.send((true, line));
        }
    });
    for (is_err, line) in line_rx {
        if is_err { eprintln!("  | {}", line); } else { println!("  | {}", line); }
        log_lines.push(line);
    }

    let exit_ok = child.wait()?.success();
    let log_content = log_lines.join("\n") + "\n";
    let _ = std::fs::remove_file(&tmp_script);
    println!("  → {}", if exit_ok { "succeeded" } else { "FAILED" });

    // Capture to main — sync round-trip via produce queue so we get result-ref
    let mut result_ref = None;
    if let Some(capture) = &s.capture {
        let capture_path = repo_path.join(capture);
        if capture_path.is_dir() {
            let msg = s.msg.as_deref()
                .unwrap_or(&format!("sentinel results: {}", s.name))
                .to_string();
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            push_tx.send(PushJob::CommitCapture {
                paths: vec![capture_path],
                msg: msg.clone(),
                branch: main_branch.to_string(),
                reply: reply_tx,
            }).ok();
            match reply_rx.recv() {
                Ok(Ok(sha)) => {
                    println!("  → captured: {} ({})", msg, &sha[..8.min(sha.len())]);
                    result_ref = Some(sha);
                }
                Ok(Err(e)) => eprintln!("  ⚠ capture failed: {}", e),
                Err(_)     => eprintln!("  ⚠ capture: push thread gone"),
            }
        } else {
            eprintln!("  ⚠ capture dir not found: {}", capture_path.display());
        }
    }

    // Write final status to disk, queue the push
    let final_status = if exit_ok { "success" } else { "failure" };
    let completed = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let content = std::fs::read_to_string(&wt_path)?;
    let mut fields: Vec<(&str, &str)> = vec![
        ("status", final_status),
        ("completed", completed.as_str()),
    ];
    let result_str;
    if let Some(ref sha) = result_ref {
        result_str = sha.clone();
        fields.push(("result-ref", result_str.as_str()));
    }
    let mut final_content = set_fields(&content, &fields);
    final_content.push_str(&format!("\n# --- log ---\n{}",
        log_content.lines()
            .map(|l| format!("# {}\n", l))
            .collect::<String>()));

    std::fs::write(&wt_path, &final_content)?;
    push_tx.send(PushJob::UpdateSentinel {
        name: s.name.clone(),
        content: final_content,
        commit_msg: format!("sentinel: {} {}", final_status, s.name),
    }).ok();

    println!("  → sentinel {} queued for push ✓", final_status);
    Ok(())
}

fn mark_failed(repo_path: &Path, name: &str, error: &str, push_tx: &mpsc::Sender<PushJob>) {
    let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(name);
    if let Ok(content) = std::fs::read_to_string(&wt_path) {
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut patched = set_fields(&content, &[
            ("status", "failure"),
            ("completed", &ts),
        ]);
        patched.push_str(&format!("\n# --- log ---\n# ERROR: {}\n", error));
        let _ = std::fs::write(&wt_path, &patched);
        push_tx.send(PushJob::UpdateSentinel {
            name: name.to_string(),
            content: patched,
            commit_msg: format!("sentinel: failure (error) {}", name),
        }).ok();
    }
}

fn reap_abandoned(
    backend: &dyn Backend,
    repo_path: &Path,
    push_tx: &mpsc::Sender<PushJob>,
    timeout_secs: i64,
) -> Result<()> {
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
            let _ = std::fs::write(&wt_path, &patched);
            push_tx.send(PushJob::UpdateSentinel {
                name: s.name.clone(),
                content: patched,
                commit_msg: format!("sentinel: abandoned {} ({}s)", s.name, age),
            }).ok();
        }
    }
    Ok(())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Update the git remote URL in the repo to use the fresh token.
fn update_remote_token(repo_path: &Path, token: &str) {
    // Read current remote URL, swap in new token
    let url_out = std::process::Command::new("git")
        .arg("-C").arg(repo_path)
        .args(["remote", "get-url", "origin"])
        .output();
    if let Ok(out) = url_out {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // Replace x-access-token:<old>@ with x-access-token:<new>@
        let new_url = if let Some(at) = url.find('@') {
            let host_and_path = &url[at..];
            format!("https://x-access-token:{}{}", token, host_and_path)
        } else {
            return;
        };
        let _ = std::process::Command::new("git")
            .arg("-C").arg(repo_path)
            .args(["remote", "set-url", "origin", &new_url])
            .status();
    }
}
