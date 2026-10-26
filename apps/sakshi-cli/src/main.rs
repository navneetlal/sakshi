use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Sakshi records what happened in a call; it never interprets it.
#[derive(Debug, Parser)]
#[command(name = "sakshi-cli", version, about)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    let _cli = Cli::parse();
    Ok(())
}
