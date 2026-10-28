mod config;
mod import;
mod mcp;
mod meetings;
mod models;
mod process;
mod record;

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
    /// Record a call (a window or display, its audio, and your microphone),
    /// then process it.
    Record(record::RecordArgs),
    /// Transcribe, diarize, and extract key frames from a meeting folder.
    Process(process::ProcessArgs),
    /// Import a recording made elsewhere (Zoom, Fireflies, ...), then
    /// process it.
    Import(import::ImportArgs),
    /// List processed meetings.
    #[command(subcommand)]
    Meetings(meetings::MeetingsCommand),
    /// List, download, or add models.
    #[command(subcommand)]
    Models(models::ModelsCommand),
    /// Show or change settings.
    #[command(subcommand)]
    Config(config::ConfigCommand),
    /// Connect MCP clients such as Claude Desktop to Sakshi.
    #[command(subcommand)]
    Mcp(mcp::McpCommand),
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
        Command::Record(args) => record::run(&data, args),
        Command::Process(args) => process::run(&data, args),
        Command::Import(args) => import::run(&data, args),
        Command::Meetings(cmd) => meetings::run(&data, cmd),
        Command::Models(cmd) => models::run(&data, cmd),
        Command::Config(cmd) => config::run(&data, cmd),
        Command::Mcp(cmd) => mcp::run(&data, cmd),
    }
}
