//! Developer tasks, run with `cargo xtask <command>`. Nothing here ships in
//! the app; it only prepares a development checkout.

mod fixtures;
mod sidecar;
mod util;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Sakshi developer tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Download the pinned nemo-speech release for this platform into
    /// `target/sidecars/`, verifying its SHA-256.
    FetchSidecar {
        /// Build variant: cpu, metal, cuda, cuda12, cuda13, or vulkan.
        /// Defaults to metal on Apple Silicon and cpu elsewhere.
        #[arg(long)]
        variant: Option<String>,
        /// Download and extract again even if the sidecar is present.
        #[arg(long)]
        force: bool,
    },
    /// Regenerate `fixtures/` from pinned AMI and FLEURS sources (about
    /// 400 MB downloaded once into `target/fixture-sources/`). Needs ffmpeg.
    Fixtures,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::FetchSidecar { variant, force } => sidecar::fetch(variant.as_deref(), force),
        Command::Fixtures => fixtures::generate(),
    }
}
