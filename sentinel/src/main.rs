mod backend;
mod commands;
mod github_token;
mod sentinel;

use anyhow::Result;
use backend::GitShellBackend;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claude-sentinel", about = "Sentinel queue manager for claude-deploy", version)]
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
        #[arg(long)] all: bool,
        #[arg(long)] log: Option<String>,
    },
    /// Watch for new sentinels and run them; also tracks origin/main (use --no-commands to disable sentinel execution)
    Watch {
        #[arg(long, default_value = "true")] commands: bool,
        #[arg(long, default_value = "5")] interval: u64,
    },
    /// Create a new sentinel and push it
    Create {
        script: PathBuf,
        #[arg(long)] capture: Option<String>,
        #[arg(long)] msg: Option<String>,
    },
    /// Mark stuck sentinels as abandoned
    Reap {
        #[arg(long, default_value = "600")] timeout: u64,
    },
    /// Remove old completed sentinels
    Prune {
        #[arg(long)] dry_run: bool,
        #[arg(long, default_value = "30")]  keep_failed: u64,
        #[arg(long, default_value = "90")]  keep_success_with_ref: u64,
        #[arg(long, default_value = "7")]   keep_success_ephemeral: u64,
        #[arg(long, default_value = "14")]  keep_abandoned: u64,
        #[arg(long)] keep_reachable: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = cli.repo
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    let backend = GitShellBackend::new(&repo_path);

    match cli.command {
        Command::Queue { all, log } =>
            commands::queue::run(&backend, all, log),
        Command::Watch { commands, interval } =>
            commands::watch::run(&backend, repo_path, commands, interval).await,
        Command::Create { script, capture, msg } =>
            commands::create::run(&backend, repo_path, script, capture, msg),
        Command::Reap { timeout } =>
            commands::reap::run(&backend, timeout, &repo_path),
        Command::Prune { dry_run, keep_failed, keep_success_with_ref,
                         keep_success_ephemeral, keep_abandoned, keep_reachable } =>
            commands::prune::run(&backend, dry_run, keep_failed, keep_success_with_ref,
                                 keep_success_ephemeral, keep_abandoned, keep_reachable),
    }
}
