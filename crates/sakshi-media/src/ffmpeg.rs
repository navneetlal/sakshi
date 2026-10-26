use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::MediaError;

/// Environment variable naming the `ffmpeg` binary to use.
pub const FFMPEG_ENV: &str = "SAKSHI_FFMPEG";

const BINARY: &str = if cfg!(windows) {
    "ffmpeg.exe"
} else {
    "ffmpeg"
};

/// A located `ffmpeg` binary. Release builds will bundle an LGPL build next
/// to the executable; development uses the one on `PATH`.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    path: PathBuf,
}

impl Ffmpeg {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$SAKSHI_FFMPEG`, then `ffmpeg` next to the running executable, then
    /// `PATH`.
    pub fn locate() -> Result<Self, MediaError> {
        if let Some(path) = std::env::var_os(FFMPEG_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::new(path));
        }
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(BINARY)))
            .filter(|p| p.is_file());
        let on_path = || {
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join(BINARY))
                    .find(|p| p.is_file())
            })
        };
        beside_exe
            .or_else(on_path)
            .map(Self::new)
            .ok_or(MediaError::FfmpegNotFound { env: FFMPEG_ENV })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A command with Sakshi's standard flags: no prompts, errors only.
    pub(crate) fn command(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error"])
            .stdin(Stdio::null());
        cmd
    }

    /// Converts any audio file ffmpeg can read into a 16 kHz mono float32
    /// WAV. Writes a temporary file and renames it, so `output` is never
    /// left half-written.
    pub fn resample_to_wav(&self, input: &Path, output: &Path) -> Result<(), MediaError> {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MediaError::io(parent, e))?;
        }
        let tmp = output.with_extension("partial.wav");
        let out = self
            .command()
            .arg("-y")
            .arg("-i")
            .arg(input)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_f32le"])
            .arg(&tmp)
            .stdout(Stdio::null())
            .output()
            .map_err(|source| MediaError::Spawn {
                path: self.path.clone(),
                source,
            })?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(MediaError::FfmpegFailed {
                input: input.to_path_buf(),
                status: out.status.to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        std::fs::rename(&tmp, output).map_err(|e| MediaError::io(output, e))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::wav_info;

    pub(crate) fn ffmpeg() -> Ffmpeg {
        Ffmpeg::locate().expect("tests need ffmpeg on PATH or $SAKSHI_FFMPEG")
    }

    #[test]
    fn resamples_stereo_48k_to_16k_mono_float() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.wav");
        let status = ffmpeg()
            .command()
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
            ])
            .args(["-ac", "2"])
            .arg(&input)
            .status()
            .unwrap();
        assert!(status.success());

        let output = dir.path().join("work").join("out.wav");
        ffmpeg().resample_to_wav(&input, &output).unwrap();

        let info = wav_info(&output).unwrap();
        assert_eq!((info.sample_rate, info.channels), (16_000, 1));
        assert!(info.float && info.bits_per_sample == 32);
        assert!((1990..=2010).contains(&info.duration_ms), "{info:?}");
        assert!(!output.with_extension("partial.wav").exists());
    }

    #[test]
    fn reports_ffmpeg_errors() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("garbage.wav");
        std::fs::write(&input, b"definitely not audio").unwrap();

        let err = ffmpeg()
            .resample_to_wav(&input, &dir.path().join("out.wav"))
            .unwrap_err();

        assert!(matches!(err, MediaError::FfmpegFailed { .. }), "{err}");
        assert!(!dir.path().join("out.wav").exists());
    }
}
