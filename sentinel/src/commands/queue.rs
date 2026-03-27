use crate::backend::Backend;
use crate::sentinel;
use anyhow::Result;

pub fn run(backend: &dyn Backend, show_all: bool, log_target: Option<String>) -> Result<()> {
    let sentinels = sentinel::read_all(backend)?;

    if sentinels.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }

    if let Some(target) = log_target {
        let s = sentinels.iter()
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

    let filtered: Vec<_> = sentinels.iter()
        .filter(|s| show_all || s.status.is_active())
        .collect();

    if filtered.is_empty() {
        println!("{}", if show_all {
            "No sentinels found."
        } else {
            "No active sentinels. Use --all to see completed."
        });
        return Ok(());
    }

    let name_w = filtered.iter().map(|s| s.name.len()).max().unwrap_or(20).max(8);
    let msg_w  = 32usize;

    println!("{:<11} {:<nw$} {:<19} {:<8} {:<8} {}",
        "STATUS", "SENTINEL", "CREATED", "REF", "RESULT", "MSG", nw = name_w);
    println!("{} {} {} {} {} {}",
        "-".repeat(11), "-".repeat(name_w), "-".repeat(19),
        "-".repeat(8), "-".repeat(8), "-".repeat(msg_w));

    for s in &filtered {
        let status_col = format!("{} {:<9}", s.status.icon(), s.status.to_string());
        let created   = s.created.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
            .unwrap_or_else(|| "-".into());
        let main_ref  = s.main_ref.as_deref().map(|r| &r[..r.len().min(8)]).unwrap_or("-");
        let result    = s.result_ref.as_deref().map(|r| &r[..r.len().min(8)]).unwrap_or("-");
        let msg       = s.msg.as_deref().unwrap_or("-");
        let msg_t     = if msg.len() > msg_w { format!("{}…", &msg[..msg_w-1]) }
                        else { msg.to_string() };

        println!("{:<11} {:<nw$} {:<19} {:<8} {:<8} {}",
            status_col, s.name, created, main_ref, result, msg_t, nw = name_w);
    }
    Ok(())
}
