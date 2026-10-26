//! Developer tasks, run with `cargo xtask <command>`. Nothing here ships in
//! the app; it only prepares a development checkout.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Sakshi developer tasks")]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    Ok(())
}
