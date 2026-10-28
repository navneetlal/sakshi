//! Stages what the desktop app carries besides its own binary, for
//! `pnpm run bundle`: the nemo-speech sidecar (with only the libraries it
//! loads) and a release build of `sakshi-mcp`. FFmpeg isn't bundled; users
//! install it (DECISIONS.md D29).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use sakshi_asr::NEMO_SPEECH_VERSION;

use crate::util::{run, workspace_root};

pub fn prepare() -> anyhow::Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("the app bundle is macOS-only for now; Windows and Linux follow with M5 and M6");
    }
    let root = workspace_root();
    let stage = root.join("apps/sakshi-desktop/src-tauri/bundle");
    if stage.exists() {
        std::fs::remove_dir_all(&stage)?;
    }

    // Tauri's externalBin wants `<name>-<target triple>`.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    run(Command::new(&cargo)
        .args(["build", "--release", "--locked", "-p", "sakshi-mcp-server"])
        .current_dir(&root))?;
    let triple = host_triple()?;
    let binaries = stage.join("binaries");
    std::fs::create_dir_all(&binaries)?;
    let mcp = binaries.join(format!("sakshi-mcp-{triple}"));
    std::fs::copy(root.join("target/release/sakshi-mcp"), &mcp).context("copying sakshi-mcp")?;

    let sidecar = dev_sidecar(&root)?;
    let dest = stage.join("nemo-speech");
    copy_file(
        &sidecar.join("bin/nemo-speech"),
        &dest.join("bin/nemo-speech"),
    )?;
    let libs = linked_libraries(&sidecar)?;
    for name in &libs {
        // Copying follows the version symlinks, so each name the loader
        // asks for is a real file and nothing else is carried along.
        copy_file(
            &sidecar.join("lib").join(name),
            &dest.join("lib").join(name),
        )?;
    }
    for dir in ["share/nemo-speech", "share/licenses"] {
        copy_dir(&sidecar.join(dir), &dest.join(dir))?;
    }

    println!("staged in {}:", stage.display());
    println!("  binaries/sakshi-mcp-{triple}");
    println!("  nemo-speech/ (bin, {} libraries, share)", libs.len());
    Ok(())
}

/// Rewrites this version's DMG without the signature Tauri gave it.
/// Gatekeeper checks a signed disk image when a downloaded one is opened,
/// and without notarization refuses it outright; an unsigned one opens,
/// and the app inside keeps its own signature (DECISIONS.md D38).
pub fn finish() -> anyhow::Result<()> {
    let dir = workspace_root().join("target/release/bundle/dmg");
    let prefix = format!("Sakshi_{}_", env!("CARGO_PKG_VERSION"));
    let dmgs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".dmg") && !name.ends_with(".unsigned.dmg")
        })
        .collect();
    if dmgs.is_empty() {
        bail!(
            "no {prefix}*.dmg in {}; run `pnpm run bundle`",
            dir.display()
        );
    }
    for dmg in dmgs {
        let unsigned = dmg.with_extension("unsigned.dmg");
        if unsigned.exists() {
            std::fs::remove_file(&unsigned)?;
        }
        // Converting copies the image's contents into a new, unsigned one.
        run(Command::new("hdiutil")
            .args(["convert", "-quiet"])
            .arg(&dmg)
            .args(["-format", "UDZO", "-imagekey", "zlib-level=9", "-o"])
            .arg(&unsigned))?;
        std::fs::rename(&unsigned, &dmg)?;
        println!("{}: unsigned the disk image", dmg.display());
    }
    Ok(())
}

fn host_triple() -> anyhow::Result<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .context("running rustc -vV")?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .context("rustc -vV printed no host triple")
}

/// The sidecar `cargo xtask fetch-sidecar` put in `target/sidecars/`.
fn dev_sidecar(root: &Path) -> anyhow::Result<PathBuf> {
    let prefix = format!("nemo-speech-{NEMO_SPEECH_VERSION}-macos-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(root.join("target/sidecars"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .map(|e| e.path())
        .filter(|p| p.join("bin/nemo-speech").is_file())
        .collect();
    found.sort();
    // Prefer the Metal build on Apple Silicon.
    found
        .iter()
        .find(|p| p.to_string_lossy().ends_with("-metal"))
        .or(found.first())
        .cloned()
        .context("no nemo-speech sidecar in target/sidecars; run `cargo xtask fetch-sidecar` first")
}

/// The `@rpath` libraries the sidecar loads, directly or through each
/// other, as the names the loader looks for in `lib/`.
fn linked_libraries(sidecar: &Path) -> anyhow::Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut queue = vec![sidecar.join("bin/nemo-speech")];
    while let Some(file) = queue.pop() {
        for name in rpath_references(&file)? {
            if found.insert(name.clone()) {
                let lib = sidecar.join("lib").join(&name);
                if !lib.exists() {
                    bail!("{} needs {name}, which isn't in lib/", file.display());
                }
                queue.push(lib);
            }
        }
    }
    Ok(found)
}

fn rpath_references(file: &Path) -> anyhow::Result<Vec<String>> {
    let out = Command::new("otool")
        .arg("-L")
        .arg(file)
        .output()
        .context("running otool (install the Xcode command line tools)")?;
    if !out.status.success() {
        bail!("otool -L {} failed", file.display());
    }
    Ok(parse_otool(&String::from_utf8_lossy(&out.stdout)))
}

/// Library names from `otool -L` lines such as
/// `\t@rpath/libggml.0.dylib (compatibility version 0.0.0, ...)`.
fn parse_otool(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| l.trim().strip_prefix("@rpath/"))
        .filter_map(|l| l.split(" (").next())
        .map(str::to_owned)
        .collect()
}

fn copy_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rpath_libraries_from_otool_output() {
        let output = "/x/bin/nemo-speech:\n\
            \t/System/Library/Frameworks/CoreAudio.framework/Versions/A/CoreAudio (compatibility version 1.0.0, current version 1.0.0)\n\
            \t@rpath/libnemo_speech_asr.dylib (compatibility version 0.0.0, current version 0.0.0)\n\
            \t@rpath/libggml.0.dylib (compatibility version 0.0.0, current version 0.25.1)\n\
            \t/usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 2000.0.0)\n";
        assert_eq!(
            parse_otool(output),
            ["libnemo_speech_asr.dylib", "libggml.0.dylib"]
        );
    }
}
