use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Something that can be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum SourceId {
    Window(u32),
    Display(u32),
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window(id) => write!(f, "window {id}"),
            Self::Display(id) => write!(f, "display {id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    /// Window title, or a display description.
    pub title: String,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    /// Size in points.
    pub width: u32,
    pub height: u32,
    /// The meeting app this looks like ("Zoom", "Microsoft Teams",
    /// "Google Meet"), if any.
    pub meeting: Option<String>,
    /// False for windows that are minimized, hidden, or on another desktop
    /// (including full-screen apps' own desktops).
    pub on_screen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    /// Video is scaled down to fit within these, keeping its aspect ratio.
    pub max_width: u32,
    pub max_height: u32,
    pub fps: u32,
    /// Record the microphone as the `mic` track.
    pub microphone: bool,
    /// Record the source app's audio (or the system's) as the `system` track.
    pub system_audio: bool,
    /// ffmpeg video encoder.
    pub video_codec: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            max_width: 1280,
            max_height: 720,
            fps: 15,
            microphone: true,
            system_audio: true,
            video_codec: if cfg!(target_os = "macos") {
                "h264_videotoolbox".into()
            } else {
                "libx264".into()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    /// The user hasn't been asked yet.
    NotDetermined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Permissions {
    pub screen: PermissionState,
    pub microphone: PermissionState,
}

/// What a finished recording produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStats {
    pub duration_ms: u64,
    pub video_frames: u64,
    pub system_samples: u64,
    pub mic_samples: u64,
    /// Where the recorded window sent no new video because it was out of
    /// view; the recording holds its last picture there. Audio is complete.
    #[serde(default)]
    pub video_pauses: Vec<crate::VideoPause>,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("recording is not supported on this platform yet ({0})")]
    Unsupported(&'static str),

    #[error("{0}")]
    PermissionDenied(String),

    #[error("no {0} to record; run `sakshi-cli record --list` to see what can be recorded")]
    SourceNotFound(String),

    #[error("{0}")]
    Platform(String),

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{}: {source}", path.display())]
    Wav { path: PathBuf, source: hound::Error },

    #[error("video encoder: {0}")]
    Encoder(String),

    #[error(transparent)]
    Media(#[from] sakshi_media::MediaError),

    #[error(transparent)]
    Core(#[from] sakshi_core::Error),

    #[error("{}: nothing to finalize ({reason})", dir.display())]
    NothingToFinalize { dir: PathBuf, reason: String },
}

impl CaptureError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// The largest even size within `max` that keeps `(width, height)`'s aspect
/// ratio, never scaling up. Video encoders need even dimensions.
pub fn fit_within(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let (w, h) = (f64::from(width.max(2)), f64::from(height.max(2)));
    let scale = (f64::from(max_width) / w)
        .min(f64::from(max_height) / h)
        .min(1.0);
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    (even(w * scale), even(h * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_and_keeps_aspect_ratio() {
        assert_eq!(fit_within(2560, 1440, 1280, 720), (1280, 720));
        assert_eq!(fit_within(1920, 1200, 1280, 720), (1152, 720));
        assert_eq!(
            fit_within(800, 600, 1280, 720),
            (800, 600),
            "never upscales"
        );
        assert_eq!(fit_within(1001, 703, 1280, 720), (1000, 702), "even sizes");
        assert_eq!(fit_within(3000, 400, 1280, 720), (1280, 170));
    }

    #[test]
    fn source_ids_serialize_tagged() {
        let json = serde_json::to_string(&SourceId::Window(42)).unwrap();
        assert_eq!(json, r#"{"kind":"window","id":42}"#);
    }
}
