mod commands;
mod sentinel;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "claude-deploy-sentinel",
    about = "Sentinel queue manager for claude-deploy",
    version
)]
struct Cli {
    /// Path to git repo (default: current directory)
    #[arg(short, long, global = true)]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Watch repo for new sentinels and run them
    Watch {
        /// Also execute sentinel scripts (default: observe only)
        #[arg(long)]
        commands: bool,

        /// Poll interval in seconds
        #[arg(long, default_value = "5")]
        interval: u64,
    },

    /// List sentinels in the queue
    Queue {
        /// Include completed sentinels (success/failure/abandoned)
        #[arg(long)]
        all: bool,

        /// Dump full log for a sentinel (by name or partial match)
        #[arg(long)]
        log: Option<String>,
    },

    /// Create a new sentinel and push it
    Create {
        /// Script file to run (or '-' for stdin)
        script: PathBuf,

        /// Path to capture and commit to main after run
        #[arg(long)]
        capture: Option<String>,

        /// Commit message for captured results
        #[arg(long)]
        msg: Option<String>,
    },

    /// Mark abandoned any sentinels stuck in running/claiming beyond timeout
    Reap {
        /// Timeout in seconds (default: 600)
        #[arg(long, default_value = "600")]
        timeout: u64,
    },

    /// Remove old completed sentinels
    Prune {
        /// Show what would be pruned without deleting
        #[arg(long)]
        dry_run: bool,

        /// Days to keep failed sentinels (default: 30)
        #[arg(long, default_value = "30")]
        keep_failed: u64,

        /// Days to keep successful sentinels with a result-ref (default: 90)
        #[arg(long, default_value = "90")]
        keep_success_with_ref: u64,

        /// Days to keep successful sentinels without a result-ref (default: 7)
        #[arg(long, default_value = "7")]
        keep_success_ephemeral: u64,

        /// Days to keep abandoned sentinels (default: 14)
        #[arg(long, default_value = "14")]
        keep_abandoned: u64,

        /// Never prune sentinels whose result-ref is reachable from main
        #[arg(long)]
        keep_reachable: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let repo_path = cli
        .repo
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    match cli.command {
        Command::Watch { commands, interval } => {
            commands::watch::run(repo_path, commands, interval).await
        }
        Command::Queue { all, log } => {
            commands::queue::run(repo_path, all, log)
        }
        Command::Create { script, capture, msg } => {
            commands::create::run(repo_path, script, capture, msg)
        }
        Command::Reap { timeout } => {
            commands::reap::run(repo_path, timeout)
        }
        Command::Prune {
            dry_run,
            keep_failed,
            keep_success_with_ref,
            keep_success_ephemeral,
            keep_abandoned,
            keep_reachable,
        } => commands::prune::run(
            repo_path,
            dry_run,
            keep_failed,
            keep_success_with_ref,
            keep_success_ephemeral,
            keep_abandoned,
            keep_reachable,
        ),
    }
}
