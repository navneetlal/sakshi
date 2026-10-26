//! `sakshi-mcp`: the MCP server that clients such as Claude Desktop,
//! Claude Code, Codex, and Cursor launch. Speaks MCP over stdio by default,
//! or streamable HTTP on 127.0.0.1 with `--http`.

use std::io::Write;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use rmcp::ServiceExt;
use sakshi_core::DataDir;
use sakshi_mcp::{SakshiMcp, generate_token, http_router};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "sakshi-mcp", version, about = "Sakshi MCP server")]
struct Cli {
    /// Data directory. Defaults to $SAKSHI_DATA_DIR, then the platform's
    /// application data folder.
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    /// Serve streamable HTTP on 127.0.0.1 instead of stdio.
    #[arg(long)]
    http: bool,

    /// HTTP port (0 picks a free one).
    #[arg(long, default_value_t = 8790, requires = "http")]
    port: u16,

    /// Read the bearer token from this file, creating it with a new random
    /// token (readable only by you) if it doesn't exist. Without it, a new
    /// token is printed at every start.
    #[arg(long, value_name = "PATH", requires = "http")]
    token_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout carries the protocol; logs go to stderr only.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let cli = Cli::parse();
    let data = match cli.data_dir {
        Some(dir) => DataDir::new(dir),
        None => DataDir::from_env_or_default()?,
    };
    let server = SakshiMcp::new(&data);

    if cli.http {
        let token = match &cli.token_file {
            Some(path) => token_from_file(path)?,
            None => generate_token().context("generating a token")?,
        };
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, cli.port))
            .await
            .with_context(|| format!("binding 127.0.0.1:{}", cli.port))?;
        let addr = listener.local_addr()?;
        eprintln!("Sakshi MCP server on http://{addr}/mcp");
        match &cli.token_file {
            Some(path) => eprintln!("Bearer token in {}", path.display()),
            None => eprintln!("Bearer token: {token}"),
        }
        axum::serve(listener, http_router(server, token))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
    } else {
        server
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
    }
    Ok(())
}

/// Reads the token, or writes a new one readable only by the user.
fn token_from_file(path: &Path) -> anyhow::Result<String> {
    if path.exists() {
        let token = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?
            .trim()
            .to_owned();
        anyhow::ensure!(
            token.len() >= 32,
            "{} holds a token shorter than 32 characters",
            path.display()
        );
        return Ok(token);
    }
    let token = generate_token().context("generating a token")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    writeln!(file, "{token}")?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets/token");

        let first = token_from_file(&path).unwrap();
        let second = token_from_file(&path).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn short_tokens_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "short\n").unwrap();
        assert!(token_from_file(&path).is_err());
    }
}
