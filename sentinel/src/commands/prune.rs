use crate::backend::Backend;
use crate::sentinel::{self, Status};
use anyhow::Result;
use chrono::Utc;

pub fn run(
    backend: &dyn Backend,
    dry_run: bool,
    keep_failed: u64,
    keep_success_with_ref: u64,
    keep_success_ephemeral: u64,
    keep_abandoned: u64,
    _keep_reachable: bool,
) -> Result<()> {
    backend.fetch_sentinel_branch()?;
    let sentinels = sentinel::read_all(backend)?;
    let now = Utc::now();

    let mut to_prune: Vec<String> = Vec::new();

    for s in &sentinels {
        let keep_days = match &s.status {
            Status::Success => {
                if s.result_ref.is_some() { keep_success_with_ref }
                else { keep_success_ephemeral }
            }
            Status::Failure   => keep_failed,
            Status::Abandoned => keep_abandoned,
            _ => continue, // never prune active sentinels
        };

        let age_days = s.completed
            .map(|t| (now - t).num_days())
            .unwrap_or(0);

        if age_days > keep_days as i64 {
            to_prune.push(s.name.clone());
        }
    }

    if to_prune.is_empty() {
        println!("Nothing to prune.");
        return Ok(());
    }

    for name in &to_prune {
        if dry_run { println!("would prune: {}", name); }
        else       { println!("pruning: {}", name); }
    }

    if !dry_run {
        // TODO: delete files from sentinel branch and push
        // Requires a new backend method: delete_sentinels(&[names])
        println!("(prune delete not yet implemented — run with --dry-run to preview)");
    }

    Ok(())
}
