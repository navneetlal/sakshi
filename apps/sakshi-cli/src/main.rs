mod models;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use sakshi_core::DataDir;
use tracing_subscriber::EnvFilter;

/// Sakshi records what happened in a call; it never interprets it.
#[derive(Debug, Parser)]
#[command(name = "sakshi-cli", version, about)]
struct Cli {
    /// Data directory. Defaults to $SAKSHI_DATA_DIR, then the platform's
    /// application data folder.
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List, download, or add models.
    #[command(subcommand)]
    Models(models::ModelsCommand),
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let data = match cli.data_dir {
        Some(dir) => DataDir::new(dir),
        None => DataDir::from_env_or_default()?,
    };
    match cli.command {
        Command::Models(cmd) => models::run(&data, cmd),
    }
}
