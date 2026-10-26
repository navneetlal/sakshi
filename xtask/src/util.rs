use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

/// The workspace root, one level above this crate.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
        .to_path_buf()
}

/// Downloads `url` to `dest` with the system `curl`, following redirects.
pub fn download(url: &str, dest: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    println!("downloading {url}");
    let partial = dest.with_extension("partial");
    run(Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--silent",
            "--show-error",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(url))
    .with_context(|| format!("downloading {url}"))?;
    std::fs::rename(&partial, dest)?;
    Ok(())
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Fails unless `path` hashes to `expected` (lowercase hex).
pub fn verify_sha256(path: &Path, expected: &str) -> anyhow::Result<()> {
    let actual = sha256_file(path)?;
    if actual != expected {
        bail!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        );
    }
    Ok(())
}

/// Runs a command, failing with its exit status if it does not succeed.
pub fn run(command: &mut Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .with_context(|| format!("starting {:?}", command.get_program()))?;
    if !status.success() {
        bail!("{:?} exited with {status}", command.get_program());
    }
    Ok(())
}
