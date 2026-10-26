use std::path::{Path, PathBuf};

use crate::{AsrError, NEMO_SPEECH_VERSION};

/// Environment variable naming the `nemo-speech` binary to use.
pub const SIDECAR_ENV: &str = "SAKSHI_NEMO_SPEECH";

const BINARY: &str = if cfg!(windows) {
    "nemo-speech.exe"
} else {
    "nemo-speech"
};

/// Finds the pinned `nemo-speech` binary:
///
/// 1. `$SAKSHI_NEMO_SPEECH`;
/// 2. `nemo-speech/bin/` next to the running executable, or in
///    `../Resources/` from it (inside the macOS app bundle);
/// 3. `sidecars/nemo-speech-<version>-*/bin/` in the executable's directory
///    or up to three levels above it, which is where `cargo xtask
///    fetch-sidecar` puts it in a development checkout.
pub fn locate_sidecar() -> Result<PathBuf, AsrError> {
    if let Some(path) = std::env::var_os(SIDECAR_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    exe_dir
        .and_then(find_near)
        .ok_or(AsrError::SidecarNotFound {
            expected: NEMO_SPEECH_VERSION.to_owned(),
            env: SIDECAR_ENV,
        })
}

fn find_near(exe_dir: &Path) -> Option<PathBuf> {
    let resources = exe_dir.parent().map(|contents| contents.join("Resources"));
    let bundled = std::iter::once(exe_dir.to_path_buf())
        .chain(resources)
        .map(|dir| dir.join("nemo-speech").join("bin").join(BINARY))
        .find(|p| p.is_file());
    if bundled.is_some() {
        return bundled;
    }
    let prefix = format!("nemo-speech-{NEMO_SPEECH_VERSION}-");
    for dir in exe_dir.ancestors().take(4) {
        let Ok(entries) = std::fs::read_dir(dir.join("sidecars")) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .map(|e| e.path().join("bin").join(BINARY))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        if let Some(path) = found.into_iter().next() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dev_sidecar_above_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let exe_dir = root.path().join("target").join("debug").join("deps");
        std::fs::create_dir_all(&exe_dir).unwrap();
        assert_eq!(find_near(&exe_dir), None);

        let bin = root
            .path()
            .join("target/sidecars")
            .join(format!(
                "nemo-speech-{NEMO_SPEECH_VERSION}-macos-aarch64-metal"
            ))
            .join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(BINARY), b"").unwrap();

        assert_eq!(find_near(&exe_dir), Some(bin.join(BINARY)));
    }

    #[test]
    fn finds_the_copy_inside_a_macos_app() {
        let root = tempfile::tempdir().unwrap();
        let contents = root.path().join("Sakshi.app/Contents");
        let exe_dir = contents.join("MacOS");
        let bin = contents.join("Resources/nemo-speech/bin");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(BINARY), b"").unwrap();

        assert_eq!(find_near(&exe_dir), Some(bin.join(BINARY)));
    }

    #[test]
    fn ignores_other_versions() {
        let root = tempfile::tempdir().unwrap();
        let bin = root
            .path()
            .join("sidecars/nemo-speech-0.0.1-linux-x86_64-cpu/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(BINARY), b"").unwrap();
        assert_eq!(find_near(root.path()), None);
    }
}
