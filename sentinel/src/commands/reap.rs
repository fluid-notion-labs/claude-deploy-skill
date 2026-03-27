use crate::backend::Backend;
use crate::sentinel::{self, Status};
use crate::backend::set_fields;
use anyhow::Result;
use chrono::Utc;
use crate::sentinel::SENTINEL_BRANCH;

pub fn run(backend: &dyn Backend, timeout_secs: u64) -> Result<()> {
    backend.fetch_sentinel_branch()?;
    let sentinels = sentinel::read_all(backend)?;
    let orig = backend.current_branch()?;
    let timeout = timeout_secs as i64;
    let mut reaped = 0;

    for s in sentinels.iter().filter(|s| matches!(s.status, Status::Running | Status::Claiming)) {
        let age = s.age_secs().unwrap_or(0);
        if age <= timeout { continue; }

        let prev_worker = s.worker.as_deref().unwrap_or("unknown");
        println!("⚠  abandoned: {} ({}s, worker: {})", s.name, age, prev_worker);

        backend.checkout(SENTINEL_BRANCH)?;
        if !backend.pull_sentinel_ff()? {
            eprintln!("  ⚠ could not pull sentinel branch — skipping");
            let _ = backend.checkout(&orig);
            continue;
        }

        let repo_path = std::env::current_dir()?;
        let path = repo_path.join(&s.name);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let mut patched = set_fields(&content, &[
                ("status", "abandoned"),
                ("abandoned", ts.as_str()),
            ]);
            patched.push_str(&format!(
                "\n# --- abandoned ---\n# Stuck {}s. Previous worker: {}\n",
                age, prev_worker
            ));
            let _ = backend.update_sentinel_checked_out(&s.name, &patched,
                &format!("sentinel: abandoned {} (stuck {}s)", s.name, age));
        }

        let _ = backend.checkout(&orig);
        reaped += 1;
    }

    if reaped == 0 { println!("No abandoned sentinels found."); }
    else           { println!("Reaped {} sentinel(s).", reaped); }
    Ok(())
}
