//! Recording a call: video of one window or display, the meeting app's audio
//! (the `system` track), and the microphone (the `mic` track), all timed
//! against one session clock and written crash-safely into
//! `<meeting>/capture/`. [`finalize`] turns that into `recording.mp4`,
//! `mic.wav`, and `system.wav` for the pipeline.
//!
//! Platform code sits behind [`Capture`]; [`platform`] returns the backend
//! for the running OS, or one that reports capture as unsupported.

mod audio;
mod finalize;
mod meeting_apps;
mod types;
mod video;

#[cfg(target_os = "macos")]
mod macos;

use std::time::Duration;

use sakshi_core::MeetingDir;
use sakshi_media::Ffmpeg;

pub use audio::{AudioChunk, AudioRecorder, AudioTrackWriter, downmix};
pub use finalize::{CaptureManifest, capture_dir, finalize, manifest_path, needs_finalize};
pub use meeting_apps::meeting_label;
pub use types::*;
pub use video::{FrameSlot, VideoEncoder, VideoFrame, VideoRecorder};

/// A platform's recording backend.
pub trait Capture {
    fn permissions(&self) -> Permissions;

    /// Asks the OS for screen recording access. On macOS this shows the
    /// system prompt once; afterwards the user must change it in System
    /// Settings. Returns whether access is granted now.
    fn request_screen_permission(&self) -> bool;

    /// Asks for microphone access, waiting for the user's answer.
    fn request_microphone_permission(&self) -> PermissionState;

    /// Windows and displays that can be recorded, likely meetings first.
    fn sources(&self) -> Result<Vec<Source>, CaptureError>;

    /// Starts recording `source` into `<meeting>/capture/`.
    fn start(
        &self,
        source: SourceId,
        config: &CaptureConfig,
        meeting: &MeetingDir,
        ffmpeg: &Ffmpeg,
    ) -> Result<Box<dyn Recording>, CaptureError>;
}

/// A recording in progress.
pub trait Recording {
    fn elapsed(&self) -> Duration;

    /// A problem that stopped capture early (e.g. the window closed).
    fn interruption(&self) -> Option<String>;

    /// Stops capturing and closes every file.
    fn stop(self: Box<Self>) -> Result<CaptureStats, CaptureError>;
}

/// The backend for this OS.
pub fn platform() -> Box<dyn Capture> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacCapture)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(Unsupported)
    }
}

/// Puts likely meetings first, then on-screen windows by app, then windows
/// that aren't on screen, then displays.
pub fn sort_sources(sources: &mut [Source]) {
    sources.sort_by_key(|s| {
        (
            s.meeting.is_none(),
            matches!(s.id, SourceId::Display(_)),
            !s.on_screen,
            s.app_name.clone().unwrap_or_default().to_lowercase(),
            s.title.to_lowercase(),
        )
    });
}

#[cfg(not(target_os = "macos"))]
struct Unsupported;

#[cfg(not(target_os = "macos"))]
const NOT_YET: &str = "Windows capture arrives in M5 and Linux in M6; macOS works today";

#[cfg(not(target_os = "macos"))]
impl Capture for Unsupported {
    fn permissions(&self) -> Permissions {
        Permissions {
            screen: PermissionState::Denied,
            microphone: PermissionState::Denied,
        }
    }

    fn request_screen_permission(&self) -> bool {
        false
    }

    fn request_microphone_permission(&self) -> PermissionState {
        PermissionState::Denied
    }

    fn sources(&self) -> Result<Vec<Source>, CaptureError> {
        Err(CaptureError::Unsupported(NOT_YET))
    }

    fn start(
        &self,
        _source: SourceId,
        _config: &CaptureConfig,
        _meeting: &MeetingDir,
        _ffmpeg: &Ffmpeg,
    ) -> Result<Box<dyn Recording>, CaptureError> {
        Err(CaptureError::Unsupported(NOT_YET))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: SourceId, app: &str, title: &str, meeting: Option<&str>) -> Source {
        Source {
            id,
            title: title.into(),
            app_name: Some(app.into()),
            bundle_id: None,
            width: 100,
            height: 100,
            meeting: meeting.map(str::to_owned),
            on_screen: true,
        }
    }

    #[test]
    fn meetings_come_first_and_displays_last() {
        let hidden = Source {
            on_screen: false,
            ..source(SourceId::Window(5), "Calendar", "Week", None)
        };
        let hidden_meeting = Source {
            on_screen: false,
            ..source(SourceId::Window(6), "zoom.us", "Zoom Meeting", Some("Zoom"))
        };
        let mut sources = vec![
            source(SourceId::Display(1), "", "Display 1", None),
            hidden,
            source(SourceId::Window(2), "TextEdit", "notes", None),
            hidden_meeting,
            source(SourceId::Window(3), "zoom.us", "Zoom Meeting", Some("Zoom")),
            source(SourceId::Window(4), "Finder", "Downloads", None),
        ];
        sort_sources(&mut sources);
        let ids: Vec<SourceId> = sources.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            [
                SourceId::Window(3),
                SourceId::Window(6),
                SourceId::Window(4),
                SourceId::Window(2),
                SourceId::Window(5),
                SourceId::Display(1)
            ]
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_platforms_say_capture_is_unsupported() {
        assert!(matches!(
            platform().sources(),
            Err(CaptureError::Unsupported(_))
        ));
    }
}
