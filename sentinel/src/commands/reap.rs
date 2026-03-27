use crate::backend::{set_fields, Backend};
use crate::sentinel::{self, Status};
use anyhow::Result;
use chrono::Utc;
use std::path::Path;

pub fn run(backend: &dyn Backend, timeout_secs: u64, repo_path: &Path) -> Result<()> {
    backend.fetch_sentinel_branch()?;
    let sentinels = sentinel::read_all(backend)?;
    let timeout = timeout_secs as i64;
    let mut reaped = 0;

    for s in sentinels.iter().filter(|s| matches!(s.status, Status::Running | Status::Claiming)) {
        let age = s.age_secs().unwrap_or(0);
        if age <= timeout { continue; }

        let prev_worker = s.worker.as_deref().unwrap_or("unknown");
        println!("⚠  abandoning: {} ({}s, worker: {})", s.name, age, prev_worker);

        let wt_path = repo_path.join(".git").join("claude-sentinel-wt").join(&s.name);
        if let Ok(content) = std::fs::read_to_string(&wt_path) {
            let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let mut patched = set_fields(&content, &[
                ("status", "abandoned"),
                ("abandoned", ts.as_str()),
            ]);
            patched.push_str(&format!(
                "\n# --- abandoned ---\n# Stuck {}s. Worker: {}\n", age, prev_worker
            ));
            backend.update_sentinel(&s.name, &patched,
                &format!("sentinel: abandoned {} ({}s)", s.name, age))?;
            reaped += 1;
        }
    }

    if reaped == 0 { println!("No abandoned sentinels."); }
    else           { println!("Reaped {} sentinel(s).", reaped); }
    Ok(())
}
