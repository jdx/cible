use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod github;
mod ingest;
mod report;

#[derive(Parser)]
#[command(name = "cible", version, about = "cible — nothing to see here yet")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pull CI history for a repo from GitHub into the local warehouse
    Ingest {
        /// Repository in owner/name form
        #[arg(long, short)]
        repo: String,
        /// Number of most recent merged PRs to ingest
        #[arg(long, default_value_t = 100)]
        prs: usize,
        /// Path to the warehouse database
        #[arg(long, default_value = "cible.db")]
        db: PathBuf,
        /// Re-ingest PRs already present in the warehouse
        #[arg(long)]
        force: bool,
    },
    /// Report CI statistics from the warehouse
    Report {
        /// Repository in owner/name form
        #[arg(long, short)]
        repo: String,
        /// Path to the warehouse database
        #[arg(long, default_value = "cible.db")]
        db: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest { repo, prs, db, force } => ingest::run(&repo, prs, &db, force),
        Command::Report { repo, db } => report::run(&repo, &db),
    }
}
