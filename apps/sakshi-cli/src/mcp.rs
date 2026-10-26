use std::path::PathBuf;

use clap::Subcommand;
use sakshi_app::mcp_setup::{
    self, Client, ClientPaths, InstallOutcome, ServerLaunch, Status, locate_mcp_server,
};
use sakshi_core::DataDir;

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Show how to connect each MCP client to Sakshi, and whether it is.
    Setup {
        /// Only this client: claude-desktop, claude-code, codex, or cursor.
        #[arg(long)]
        client: Option<Client>,

        /// Add Sakshi to the client's config file (Claude Desktop and Cursor
        /// only). The old file is kept as a backup next to it.
        #[arg(long, requires = "client")]
        write: bool,

        /// The sakshi-mcp binary clients should launch. Defaults to the one
        /// next to sakshi-cli.
        #[arg(long, value_name = "PATH")]
        server: Option<PathBuf>,
    },
}

pub fn run(data: &DataDir, cmd: McpCommand) -> anyhow::Result<()> {
    let McpCommand::Setup {
        client,
        write,
        server,
    } = cmd;
    let server = match server {
        Some(path) => std::path::absolute(path)?,
        None => locate_mcp_server()?,
    };
    let launch = ServerLaunch::new(server, data);
    let paths = ClientPaths::detect()?;

    if write {
        let client = client.expect("clap requires --client with --write");
        match mcp_setup::install(client, &launch, &paths)? {
            InstallOutcome::AlreadyConfigured => {
                println!("{} is already set up.", client.name());
            }
            InstallOutcome::Written { path, backup } => {
                println!("Added Sakshi to {}.", path.display());
                if let Some(backup) = backup {
                    println!("The previous file is at {}.", backup.display());
                }
                if client == Client::ClaudeDesktop {
                    println!("Quit and reopen Claude Desktop to use it.");
                }
            }
        }
        return Ok(());
    }

    for setup in mcp_setup::clients(&launch, &paths) {
        if client.is_some_and(|c| c != setup.client) {
            continue;
        }
        let status = match setup.status {
            Status::NotFound => "not installed",
            Status::NotConfigured => "not set up",
            Status::Different => "set up with a different command",
            Status::Configured => "set up",
            Status::Unreadable => "config file can't be read",
        };
        println!("{} ({status})", setup.name);
        println!("{}", setup.instructions);
        println!();
        for line in setup.snippet.lines() {
            println!("    {line}");
        }
        if setup.can_write && matches!(setup.status, Status::NotConfigured | Status::Different) {
            println!();
            println!(
                "Or let Sakshi do it: sakshi-cli mcp setup --client {} --write",
                setup.client
            );
        }
        println!();
    }
    Ok(())
}
