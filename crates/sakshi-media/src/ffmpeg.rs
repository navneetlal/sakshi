use std::ffi::OsStr;
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

/// Where package managers install ffmpeg. An app started from the Finder
/// or a desktop launcher doesn't get the shell's `PATH`, so these are
/// checked as well.
const INSTALL_DIRS: &[&str] = if cfg!(target_os = "macos") {
    // Homebrew on Apple Silicon, Homebrew on Intel, MacPorts.
    &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
} else if cfg!(windows) {
    &[]
} else {
    &["/usr/bin", "/usr/local/bin"]
};

/// The oldest FFmpeg release Sakshi is tested with. Releases back to 4.4
/// have every option Sakshi uses but aren't tested.
pub const MIN_FFMPEG: (u32, u32) = (8, 0);

/// What `ffmpeg -version` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegVersion {
    /// As printed, e.g. `9.0.2` or `8.0.1-3ubuntu2`.
    pub text: String,
    /// Major and minor release; development builds have none.
    pub release: Option<(u32, u32)>,
}

impl FfmpegVersion {
    /// Reads the first line of `ffmpeg -version`, such as
    /// `ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers`.
    pub fn parse(first_line: &str) -> Option<Self> {
        let text = first_line
            .strip_prefix("ffmpeg version ")?
            .split_whitespace()
            .next()?;
        let numeric: String = text
            .trim_start_matches('n')
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let mut parts = numeric.split('.').map(str::parse::<u32>);
        let release = match (parts.next(), parts.next()) {
            (Some(Ok(major)), Some(Ok(minor))) => Some((major, minor)),
            (Some(Ok(major)), None) => Some((major, 0)),
            _ => None,
        };
        Some(Self {
            text: text.to_owned(),
            release,
        })
    }

    /// False only for a release known to be older than [`MIN_FFMPEG`].
    pub fn is_supported(&self) -> bool {
        self.release.is_none_or(|release| release >= MIN_FFMPEG)
    }
}

/// A located `ffmpeg` binary. Sakshi doesn't ship one: users install
/// ffmpeg themselves (DECISIONS.md D29).
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    path: PathBuf,
}

impl Ffmpeg {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$SAKSHI_FFMPEG`, then `ffmpeg` next to the running executable, then
    /// `PATH`, then the usual package manager locations.
    pub fn locate() -> Result<Self, MediaError> {
        if let Some(path) = std::env::var_os(FFMPEG_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::new(path));
        }
        let exe = std::env::current_exe().ok();
        let exe_dir = exe.as_deref().and_then(Path::parent);
        let installs: Vec<&Path> = INSTALL_DIRS.iter().map(Path::new).collect();
        find(exe_dir, std::env::var_os("PATH").as_deref(), &installs)
            .map(Self::new)
            .ok_or(MediaError::FfmpegNotFound { env: FFMPEG_ENV })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version(&self) -> Result<FfmpegVersion, MediaError> {
        let out = Command::new(&self.path)
            .arg("-version")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| MediaError::Spawn {
                path: self.path.clone(),
                source,
            })?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout
            .lines()
            .next()
            .and_then(FfmpegVersion::parse)
            .ok_or_else(|| MediaError::NotFfmpeg {
                path: self.path.clone(),
            })
    }

    /// A command with Sakshi's standard flags: no prompts, errors only,
    /// stdin closed.
    pub fn command(&self) -> Command {
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

    /// What a media file holds, from the stream listing ffmpeg prints for
    /// an input. Fails if ffmpeg can't read it as media.
    pub fn probe(&self, input: &Path) -> Result<MediaInfo, MediaError> {
        let out = Command::new(&self.path)
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(input)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| MediaError::Spawn {
                path: self.path.clone(),
                source,
            })?;
        // Without an output file ffmpeg always exits with an error; the
        // listing is on stderr either way.
        let listing = String::from_utf8_lossy(&out.stderr);
        if !listing
            .lines()
            .any(|l| l.trim_start().starts_with("Stream #"))
        {
            return Err(MediaError::FfmpegFailed {
                input: input.to_path_buf(),
                status: "reading it as audio or video".into(),
                stderr: listing.trim().to_owned(),
            });
        }
        Ok(MediaInfo::parse(&listing))
    }
}

/// What [`Ffmpeg::probe`] found in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    /// A video stream other than cover art.
    pub has_video: bool,
    pub has_audio: bool,
    /// The container's `creation_time` as written, usually RFC 3339.
    pub creation_time: Option<String>,
}

impl MediaInfo {
    fn parse(listing: &str) -> Self {
        let mut info = Self::default();
        for line in listing.lines().map(str::trim) {
            if line.starts_with("Stream #") {
                info.has_video |= line.contains(": Video:") && !line.contains("(attached pic)");
                info.has_audio |= line.contains(": Audio:");
            } else if info.creation_time.is_none()
                && let Some(value) = line.strip_prefix("creation_time")
            {
                // The first one is the container's; streams list their own.
                info.creation_time = value
                    .trim_start()
                    .strip_prefix(':')
                    .map(|v| v.trim().to_owned())
                    .filter(|v| !v.is_empty());
            }
        }
        info
    }
}

/// The first `ffmpeg` next to the executable, on `path_var`, or in
/// `install_dirs`.
fn find(
    exe_dir: Option<&Path>,
    path_var: Option<&OsStr>,
    install_dirs: &[&Path],
) -> Option<PathBuf> {
    let on_path = path_var
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default();
    exe_dir
        .into_iter()
        .map(Path::to_path_buf)
        .chain(on_path)
        .chain(install_dirs.iter().map(|d| d.to_path_buf()))
        .map(|dir| dir.join(BINARY))
        .find(|p| p.is_file())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::wav_info;

    pub(crate) fn ffmpeg() -> Ffmpeg {
        Ffmpeg::locate().expect("tests need ffmpeg on PATH or $SAKSHI_FFMPEG")
    }

    #[test]
    fn parses_release_and_distribution_versions() {
        let parse = |line: &str| FfmpegVersion::parse(line).unwrap();
        let v = parse("ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers");
        assert_eq!((v.text.as_str(), v.release), ("9.0.2", Some((9, 0))));
        assert_eq!(
            parse("ffmpeg version 8.0.1-3ubuntu2 Copyright").release,
            Some((8, 0))
        );
        assert_eq!(parse("ffmpeg version n7.1 Copyright").release, Some((7, 1)));
        assert_eq!(
            parse("ffmpeg version 9.0.2-essentials_build-www.gyan.dev Copyright").release,
            Some((9, 0))
        );
        // Development builds have no release number and aren't rejected.
        let git = parse("ffmpeg version N-118000-gabcdef Copyright");
        assert_eq!(git.release, None);
        assert!(git.is_supported());

        assert!(parse("ffmpeg version 8.0 Copyright").is_supported());
        assert!(!parse("ffmpeg version 7.1.1 Copyright").is_supported());
        assert_eq!(FfmpegVersion::parse("avconv version 12"), None);
    }

    #[test]
    fn reports_the_installed_version() {
        let version = ffmpeg().version().unwrap();
        assert!(!version.text.is_empty());
    }

    #[test]
    fn looks_beside_the_app_then_path_then_install_dirs() {
        let root = tempfile::tempdir().unwrap();
        let dir = |name: &str| {
            let d = root.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let (app, on_path, brew, empty) = (dir("app"), dir("path"), dir("brew"), dir("empty"));
        for d in [&app, &on_path, &brew] {
            std::fs::write(d.join(BINARY), b"").unwrap();
        }
        let path_var = std::env::join_paths([&empty, &on_path]).unwrap();
        let installs = [brew.as_path()];

        let found = |exe: Option<&Path>, path: Option<&OsStr>| find(exe, path, &installs);
        assert_eq!(found(Some(&app), Some(&path_var)), Some(app.join(BINARY)));
        assert_eq!(
            found(Some(&empty), Some(&path_var)),
            Some(on_path.join(BINARY))
        );
        // An app started from the Finder: no useful PATH.
        assert_eq!(found(Some(&empty), None), Some(brew.join(BINARY)));
        assert_eq!(find(Some(&empty), None, &[empty.as_path()]), None);
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
        let err = ffmpeg().probe(&input).unwrap_err();
        assert!(matches!(err, MediaError::FfmpegFailed { .. }), "{err}");
    }

    #[test]
    fn parses_the_stream_listing() {
        let zoom = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'GMT20261020-040000_Recording.mp4':
  Metadata:
    major_brand     : mp42
    creation_time   : 2026-10-20T04:00:00.000000Z
  Duration: 00:42:10.05, start: 0.000000, bitrate: 512 kb/s
  Stream #0:0[0x1](und): Audio: aac (LC) (mp4a / 0x6134706D), 32000 Hz, mono, fltp, 64 kb/s (default)
      Metadata:
        creation_time   : 2026-10-20T04:01:00.000000Z
  Stream #0:1[0x2](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 1920x1080, 25 fps";
        assert_eq!(
            MediaInfo::parse(zoom),
            MediaInfo {
                has_video: true,
                has_audio: true,
                creation_time: Some("2026-10-20T04:00:00.000000Z".into()),
            }
        );
        let podcast = "Input #0, mp3, from 'call.mp3':
  Stream #0:0: Audio: mp3 (mp3float), 44100 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p, 600x600, 90k tbr (attached pic)";
        assert_eq!(
            MediaInfo::parse(podcast),
            MediaInfo {
                has_audio: true,
                ..MediaInfo::default()
            }
        );
    }

    #[test]
    fn probes_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("call.mp4");
        let status = ffmpeg()
            .command()
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=5:duration=1",
            ])
            .args(["-f", "lavfi", "-i", "sine=duration=1"])
            .args([
                "-metadata",
                "creation_time=2026-10-20T04:00:00Z",
                "-shortest",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let info = ffmpeg().probe(&video).unwrap();
        assert!(info.has_video && info.has_audio, "{info:?}");
        assert!(
            info.creation_time
                .unwrap()
                .starts_with("2026-10-20T04:00:00")
        );
    }
}
