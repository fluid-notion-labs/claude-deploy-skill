use crate::backend::Backend;
use crate::sentinel;
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

pub fn run(
    backend: &dyn Backend,
    repo_path: PathBuf,
    script: PathBuf,
    capture: Option<String>,
    msg: Option<String>,
) -> Result<()> {
    let script_body = if script == Path::new("-") {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(&script)
            .with_context(|| format!("read script: {}", script.display()))?
    };

    let name = sentinel::new_name(&repo_path)?;
    let main_ref = backend.head_short()?;
    let created = Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    let mut content = format!(
        "status: new\nmain-ref: {}\ncreated: {}\n",
        main_ref, created
    );
    if let Some(c) = &capture { content.push_str(&format!("capture: {}\n", c)); }
    if let Some(m) = &msg     { content.push_str(&format!("msg: {}\n", m)); }
    content.push('\n');
    content.push_str(script_body.trim_end());
    content.push('\n');

    backend.ensure_sentinel_branch()?;
    backend.push_sentinel(&name, &content, &format!("sentinel: new {}", name))?;

    println!("✓ pushed sentinel: {}", name);
    Ok(())
}
