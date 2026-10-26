//! Fetches the pinned NeMo-Speech.cpp release into `target/sidecars/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use sakshi_asr::NEMO_SPEECH_VERSION;

use crate::util::{download, run, verify_sha256, workspace_root};

/// Release archives and their SHA-256, copied from the release's
/// `SHA256SUMS`. Must match [`NEMO_SPEECH_VERSION`].
const ASSETS: &[(&str, &str, &str, &str)] = &[
    // (os, arch, variant, sha256)
    (
        "linux",
        "aarch64",
        "cpu",
        "ac50694d4729f9eae390a9851d38f51269d9af9829b535325539090a3b95c787",
    ),
    (
        "linux",
        "aarch64",
        "cuda12",
        "634c622ac77f6fea89ef605c62d684e1f7c45f0c0026be570151481b5ab2379a",
    ),
    (
        "linux",
        "aarch64",
        "cuda13",
        "733ed366109d91e7c719a2000f8e6ddc579cc86ac4d036683309e07b14c51dad",
    ),
    (
        "linux",
        "aarch64",
        "vulkan",
        "370f69391db9a8f1d112e1baaa7f1f738776bd88fd4f4e2d61f5d6489d061126",
    ),
    (
        "linux",
        "x86_64",
        "cpu",
        "f396057150f1b774935c7414fd32ebc04195a795a34120d78d8c9ecefa1b7507",
    ),
    (
        "linux",
        "x86_64",
        "cuda",
        "aef573fcc6c2c759401d2ed03e5ca9e401ed23b0e38cb852d917080c1e2d4515",
    ),
    (
        "linux",
        "x86_64",
        "vulkan",
        "c8d0ed5b9ac5fb6d5c460015ba38ea217415c44edc1e6d49120646781009aee6",
    ),
    (
        "macos",
        "aarch64",
        "cpu",
        "6a91355e15d22db6a319144d389d2e1b02f233c1c6808c0da85b6d3283e52296",
    ),
    (
        "macos",
        "aarch64",
        "metal",
        "5cb02ba7c04f0b5585ce5cde9c830be5f0b7dfb4c83083c500109c381b4f2da9",
    ),
    (
        "macos",
        "x86_64",
        "cpu",
        "d52c318d8197511d4f7eee02bcf05511f7e91b300555a130b562e571695a0f39",
    ),
    (
        "windows",
        "x86_64",
        "cpu",
        "82c80451086e86194aba9af19f800da77cd66d1c28d993d0efceffd6cdb49bb8",
    ),
    (
        "windows",
        "x86_64",
        "cuda",
        "9ffe5e842c39a7d3211f3b2c3b3aa1bca8c0b2455d96d961ed5f22ec5108f98d",
    ),
    (
        "windows",
        "x86_64",
        "vulkan",
        "edf15a04ba98740aa0ab75ae4681ac7ad0a488b22b35ce2cbebd10590bb1c1e1",
    ),
];

pub fn fetch(variant: Option<&str>, force: bool) -> anyhow::Result<()> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let variant = variant.unwrap_or(match (os, arch) {
        ("macos", "aarch64") => "metal",
        _ => "cpu",
    });
    let Some(&(_, _, _, sha256)) = ASSETS
        .iter()
        .find(|(o, a, v, _)| *o == os && *a == arch && *v == variant)
    else {
        bail!("nemo-speech {NEMO_SPEECH_VERSION} has no prebuilt {variant} build for {os}-{arch}");
    };

    let stem = format!("nemo-speech-{NEMO_SPEECH_VERSION}-{os}-{arch}-{variant}");
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    let archive_name = format!("{stem}.{ext}");
    let sidecars = workspace_root().join("target").join("sidecars");
    let dest = sidecars.join(&stem);

    if dest.exists() && !force {
        println!("already present: {}", binary_in(&dest)?.display());
        return Ok(());
    }

    let archive = sidecars.join("downloads").join(&archive_name);
    if !archive.exists() || verify_sha256(&archive, sha256).is_err() {
        let url = format!(
            "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v{NEMO_SPEECH_VERSION}/{archive_name}"
        );
        download(&url, &archive)?;
    }
    verify_sha256(&archive, sha256)?;

    extract(&archive, &sidecars.join(format!(".extract-{stem}")), &dest)?;
    println!("nemo-speech sidecar ready: {}", binary_in(&dest)?.display());
    Ok(())
}

/// Extracts into a scratch directory, then moves the archive's single
/// top-level folder (or the scratch directory itself) to `dest`.
fn extract(archive: &Path, scratch: &Path, dest: &Path) -> anyhow::Result<()> {
    for dir in [scratch, dest] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    std::fs::create_dir_all(scratch)?;
    // bsdtar (macOS, Windows 10+) reads zip as well as tar.gz.
    run(Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(scratch))
    .with_context(|| format!("extracting {}", archive.display()))?;

    let entries: Vec<PathBuf> = std::fs::read_dir(scratch)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    match entries.as_slice() {
        [single] if single.is_dir() => {
            std::fs::rename(single, dest)?;
            std::fs::remove_dir(scratch)?;
        }
        _ => std::fs::rename(scratch, dest)?,
    }
    Ok(())
}

fn binary_in(dir: &Path) -> anyhow::Result<PathBuf> {
    let name = if cfg!(windows) {
        "nemo-speech.exe"
    } else {
        "nemo-speech"
    };
    let candidates = [dir.join("bin").join(name), dir.join(name)];
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .with_context(|| format!("no {name} found under {}", dir.display()))
}
