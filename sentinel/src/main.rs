mod commands;
mod sentinel;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "claude-sentinel",
    about = "Sentinel queue manager for claude-deploy",
    version
)]
struct Cli {
    #[arg(short, long, global = true)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List sentinels in the queue
    Queue {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        log: Option<String>,
    },
    /// Watch repo for new sentinels and run them
    Watch {
        #[arg(long)]
        commands: bool,
        #[arg(long, default_value = "5")]
        interval: u64,
    },
    /// Create a new sentinel and push it
    Create {
        script: PathBuf,
        #[arg(long)]
        capture: Option<String>,
        #[arg(long)]
        msg: Option<String>,
    },
    /// Mark stuck sentinels as abandoned
    Reap {
        #[arg(long, default_value = "600")]
        timeout: u64,
    },
    /// Remove old completed sentinels
    Prune {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "30")]
        keep_failed: u64,
        #[arg(long, default_value = "90")]
        keep_success_with_ref: u64,
        #[arg(long, default_value = "7")]
        keep_success_ephemeral: u64,
        #[arg(long, default_value = "14")]
        keep_abandoned: u64,
        #[arg(long)]
        keep_reachable: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = cli
        .repo
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    match cli.command {
        Command::Queue { all, log } => commands::queue::run(repo_path, all, log),
        Command::Watch { .. }  => todo!("watch not yet implemented"),
        Command::Create { .. } => todo!("create not yet implemented"),
        Command::Reap { .. }   => todo!("reap not yet implemented"),
        Command::Prune { .. }  => todo!("prune not yet implemented"),
    }
}
