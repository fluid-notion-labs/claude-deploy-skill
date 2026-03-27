use crate::sentinel;
use anyhow::Result;
use git2::Repository;
use std::path::Path;

pub fn run(repo_path: impl AsRef<Path>, show_all: bool, log_target: Option<String>) -> Result<()> {
    let repo = Repository::open(repo_path.as_ref())?;
    let sentinels = sentinel::read_all(&repo)?;

    if sentinels.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }

    // --log mode: dump a single sentinel in full
    if let Some(target) = log_target {
        let s = sentinels
            .iter()
            .find(|s| s.name.contains(&target))
            .ok_or_else(|| anyhow::anyhow!("sentinel not found: {}", target))?;

        println!("name:       {}", s.name);
        println!("status:     {} {}", s.status.icon(), s.status);
        if let Some(r) = &s.main_ref   { println!("main-ref:   {}", r); }
        if let Some(t) = s.created     { println!("created:    {}", t.format("%Y-%m-%dT%H:%M:%S")); }
        if let Some(t) = s.ran         { println!("ran:        {}", t.format("%Y-%m-%dT%H:%M:%S")); }
        if let Some(t) = s.completed   { println!("completed:  {}", t.format("%Y-%m-%dT%H:%M:%S")); }
        if let Some(w) = &s.worker     { println!("worker:     {}", w); }
        if let Some(c) = &s.capture    { println!("capture:    {}", c); }
        if let Some(r) = &s.result_ref { println!("result-ref: {}", r); }
        if let Some(m) = &s.msg        { println!("msg:        {}", m); }

        println!("\n--- script ---");
        println!("{}", s.script_body.trim());

        println!("\n--- log ---");
        match &s.log {
            Some(log) => println!("{}", log.trim()),
            None      => println!("(no log yet)"),
        }
        return Ok(());
    }

    // Table mode
    let filtered: Vec<_> = sentinels
        .iter()
        .filter(|s| show_all || s.status.is_active())
        .collect();

    if filtered.is_empty() {
        if show_all {
            println!("No sentinels found.");
        } else {
            println!("No active sentinels. Use --all to see completed.");
        }
        return Ok(());
    }

    // Column widths
    let name_w  = filtered.iter().map(|s| s.name.len()).max().unwrap_or(20).max(8);
    let msg_w   = 32usize;

    println!(
        "{:<11} {:<name_w$} {:<19} {:<8} {:<8} {}",
        "STATUS", "SENTINEL", "CREATED", "REF", "RESULT", "MSG",
        name_w = name_w,
    );
    println!(
        "{} {} {} {} {} {}",
        "-".repeat(11), "-".repeat(name_w), "-".repeat(19),
        "-".repeat(8), "-".repeat(8), "-".repeat(msg_w),
    );

    for s in &filtered {
        let status_col = format!("{} {:<9}", s.status.icon(), s.status.to_string());
        let created    = s.created
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
            .unwrap_or_else(|| "-".into());
        let main_ref   = s.main_ref.as_deref()
            .map(|r| &r[..r.len().min(8)])
            .unwrap_or("-");
        let result_ref = s.result_ref.as_deref()
            .map(|r| &r[..r.len().min(8)])
            .unwrap_or("-");
        let msg        = s.msg.as_deref().unwrap_or("-");
        let msg_trunc  = if msg.len() > msg_w {
            format!("{}…", &msg[..msg_w - 1])
        } else {
            msg.to_string()
        };

        println!(
            "{:<11} {:<name_w$} {:<19} {:<8} {:<8} {}",
            status_col, s.name, created, main_ref, result_ref, msg_trunc,
            name_w = name_w,
        );
    }

    Ok(())
}
