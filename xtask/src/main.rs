//! Developer tasks, run with `cargo xtask <command>`. Nothing here ships in
//! the app; it only prepares a development checkout.

mod brand;
mod bundle;
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
    /// Redraw the mark in `assets/brand/`'s PNG icons from the geometry in
    /// its README, keeping each tile. Without --write, only reports.
    BrandIcons {
        #[arg(long)]
        write: bool,
    },
    /// Stage the nemo-speech sidecar and a release `sakshi-mcp` in
    /// `apps/sakshi-desktop/src-tauri/bundle/` for the app bundle (macOS).
    /// Run by `pnpm run bundle`; needs `fetch-sidecar` first.
    PrepareBundle,
    /// Strip the signature Tauri put on the DMG, which Gatekeeper refuses
    /// without notarization; the app inside stays signed. Run by `pnpm run
    /// bundle` after `tauri build`.
    FinishBundle,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::FetchSidecar { variant, force } => sidecar::fetch(variant.as_deref(), force),
        Command::Fixtures => fixtures::generate(),
        Command::BrandIcons { write } => brand::run(write),
        Command::PrepareBundle => bundle::prepare(),
        Command::FinishBundle => bundle::finish(),
    }
}
