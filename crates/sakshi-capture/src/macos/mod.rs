//! macOS capture: ScreenCaptureKit for window or display video and the
//! meeting app's audio, AVAudioEngine for the microphone.
//!
//! Every timestamp is on the host clock (`mach_absolute_time` in seconds):
//! ScreenCaptureKit stamps sample buffers with it, and AVAudioEngine's
//! `hostTime` converts to it, so all three streams share the session clock.

// Apple's frameworks are only reachable through FFI. Every unsafe block in
// this module calls the generated objc2 bindings with the arguments their
// Objective-C documentation requires.
#![allow(unsafe_code)]

mod content;
mod mic;
mod permissions;
mod stream;
mod window;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use objc2_core_media::{CMClock, CMTime};
use sakshi_core::{MeetingDir, write_json_atomic};
use sakshi_media::Ffmpeg;

use crate::finalize::{CaptureManifest, capture_dir, manifest_path};
use crate::{
    AudioRecorder, AudioTrackWriter, Capture, CaptureConfig, CaptureError, CaptureStats, FrameSlot,
    PauseTracker, PermissionState, Permissions, Recording, Source, SourceId, VideoEncoder,
    VideoPause, VideoRecorder, WindowView, fit_within,
};

/// ScreenCaptureKit's audio is requested at this rate, mono.
const SYSTEM_AUDIO_RATE: u32 = 48_000;

/// How often a recorded window is checked for paused video.
const WATCH_EVERY: Duration = Duration::from_millis(500);

pub(crate) struct MacCapture;

pub(crate) fn seconds(t: CMTime) -> Option<f64> {
    (t.timescale > 0 && t.flags.contains(objc2_core_media::CMTimeFlags::Valid))
        .then(|| t.value as f64 / f64::from(t.timescale))
}

/// Now, on the host clock.
fn host_now() -> f64 {
    let t = unsafe { CMClock::host_time_clock().time() };
    seconds(t).expect("the host clock always has a valid time")
}

impl Capture for MacCapture {
    fn permissions(&self) -> Permissions {
        Permissions {
            screen: permissions::screen(),
            microphone: permissions::microphone(),
        }
    }

    fn request_screen_permission(&self) -> bool {
        permissions::request_screen()
    }

    fn request_microphone_permission(&self) -> PermissionState {
        permissions::request_microphone()
    }

    fn sources(&self) -> Result<Vec<Source>, CaptureError> {
        let mut sources = content::sources()?;
        crate::sort_sources(&mut sources);
        Ok(sources)
    }

    fn start(
        &self,
        source: SourceId,
        config: &CaptureConfig,
        meeting: &MeetingDir,
        ffmpeg: &Ffmpeg,
    ) -> Result<Box<dyn Recording>, CaptureError> {
        MacRecording::start(source, config, meeting, ffmpeg)
            .map(|r| Box::new(r) as Box<dyn Recording>)
    }

    fn show(&self, source: SourceId) -> Result<(), CaptureError> {
        match source {
            SourceId::Window(id) => window::bring_forward(id),
            SourceId::Display(_) => Ok(()),
        }
    }
}

/// Checks the recorded window every [`WATCH_EVERY`] for video that stopped
/// because the window is out of view.
struct Watch {
    paused: Arc<Mutex<Option<WindowView>>>,
    stop: mpsc::Sender<()>,
    thread: JoinHandle<PauseTracker>,
}

impl Watch {
    fn start(window_id: u32, frames: Arc<FrameSlot>, started: Instant) -> Self {
        let paused = Arc::new(Mutex::new(None));
        let current = Arc::clone(&paused);
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("sakshi-video-watch".into())
            .spawn(move || {
                let mut tracker = PauseTracker::default();
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(WATCH_EVERY) {
                    let view = window::view(window_id);
                    let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    let received = frames.received();
                    tracing::debug!(?view, now, received, "watching the window");
                    let state = tracker.observe(now, received, view);
                    let mut current = current.lock().expect("lock");
                    if *current != state {
                        match state {
                            Some(view) => tracing::info!(?view, "video paused: window out of view"),
                            None => tracing::info!("video resumed"),
                        }
                        *current = state;
                    }
                }
                tracker
            })
            .expect("spawning the video watch thread");
        Self {
            paused,
            stop,
            thread,
        }
    }

    fn paused(&self) -> Option<WindowView> {
        *self.paused.lock().expect("lock")
    }

    fn finish(self, end_ms: u64) -> Vec<VideoPause> {
        drop(self.stop);
        match self.thread.join() {
            Ok(tracker) => tracker.finish(end_ms),
            Err(_) => {
                tracing::warn!("the video watch thread panicked; pauses weren't kept");
                Vec::new()
            }
        }
    }
}

struct MacRecording {
    stream: stream::Stream,
    mic: Option<(mic::Microphone, AudioRecorder)>,
    system: Option<AudioRecorder>,
    video: VideoRecorder,
    /// Only for windows; displays are always in view.
    watch: Option<Watch>,
    started: Instant,
    manifest: CaptureManifest,
    manifest_path: std::path::PathBuf,
}

impl MacRecording {
    fn start(
        source_id: SourceId,
        config: &CaptureConfig,
        meeting: &MeetingDir,
        ffmpeg: &Ffmpeg,
    ) -> Result<Self, CaptureError> {
        let content = content::shareable_content()?;
        let target = content::target(&content, source_id)?;
        let (width, height) = fit_within(
            target.source.width.max(2) * target.scale,
            target.source.height.max(2) * target.scale,
            config.max_width,
            config.max_height,
        );

        let dir = capture_dir(meeting);
        std::fs::create_dir_all(&dir).map_err(|e| CaptureError::io(&dir, e))?;
        let mut manifest = CaptureManifest {
            started_at: String::new(),
            source: target.source.clone(),
            width,
            height,
            fps: config.fps,
            video: "capture/video.mp4".into(),
            system_audio: config.system_audio.then(|| "capture/system.wav".into()),
            mic: config.microphone.then(|| "capture/mic.wav".into()),
            stopped_at: None,
            stats: None,
        };

        // The microphone is opened first: its rate is only known from the
        // device, and failing here leaves nothing running.
        let microphone = if config.microphone {
            Some(mic::Microphone::open()?)
        } else {
            None
        };

        let frames = Arc::new(FrameSlot::default());
        let encoder = VideoEncoder::spawn(
            ffmpeg,
            &meeting.path().join(&manifest.video),
            width,
            height,
            config.fps,
            &config.video_codec,
        )?;

        // The session starts now: both clocks are read together.
        let started = Instant::now();
        let session_start = host_now();
        manifest.started_at = Timestamp::now().to_string();
        write_json_atomic(&manifest_path(meeting), &manifest)?;

        let system = match &manifest.system_audio {
            Some(file) => Some(AudioRecorder::start(AudioTrackWriter::create(
                &meeting.path().join(file),
                SYSTEM_AUDIO_RATE,
                session_start,
            )?)),
            None => None,
        };
        let mic = match (microphone, &manifest.mic) {
            (Some(device), Some(file)) => {
                let recorder = AudioRecorder::start(AudioTrackWriter::create(
                    &meeting.path().join(file),
                    device.sample_rate(),
                    session_start,
                )?);
                device.start(recorder.sender())?;
                Some((device, recorder))
            }
            _ => None,
        };
        let video = VideoRecorder::start(encoder, Arc::clone(&frames), config.fps, started);
        let watch = match source_id {
            SourceId::Window(id) => Some(Watch::start(id, Arc::clone(&frames), started)),
            SourceId::Display(_) => None,
        };
        let stream = stream::Stream::start(
            &target,
            width,
            height,
            config.fps,
            SYSTEM_AUDIO_RATE,
            frames,
            system.as_ref().map(AudioRecorder::sender),
        )?;
        if let Some(clock) = stream.clock_offset(session_start) {
            tracing::debug!(offset_s = clock, "ScreenCaptureKit clock vs host clock");
        }
        tracing::info!(source = %source_id, width, height, fps = config.fps, "recording");

        Ok(Self {
            stream,
            mic,
            system,
            video,
            watch,
            started,
            manifest,
            manifest_path: manifest_path(meeting),
        })
    }
}

impl Recording for MacRecording {
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn interruption(&self) -> Option<String> {
        self.stream.interruption()
    }

    fn video_paused(&self) -> Option<WindowView> {
        self.watch.as_ref().and_then(Watch::paused)
    }

    fn stop(self: Box<Self>) -> Result<CaptureStats, CaptureError> {
        let Self {
            stream,
            mic,
            system,
            video,
            watch,
            started,
            mut manifest,
            manifest_path,
        } = *self;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        // Stop the sources first, so the writers see the end of their input.
        let stream_result = stream.stop();
        let mic_samples = match mic {
            Some((device, recorder)) => {
                device.stop();
                recorder.finish()?
            }
            None => 0,
        };
        let system_samples = system.map(AudioRecorder::finish).transpose()?.unwrap_or(0);
        let video_pauses = watch.map(|w| w.finish(duration_ms)).unwrap_or_default();
        let video_frames = video.stop()?;
        stream_result?;

        let stats = CaptureStats {
            duration_ms,
            video_frames,
            system_samples,
            mic_samples,
            video_pauses,
        };
        manifest.stopped_at = Some(Timestamp::now().to_string());
        manifest.stats = Some(stats.clone());
        write_json_atomic(&manifest_path, &manifest)?;
        Ok(stats)
    }
}
