//! Recording runs on its own thread, which owns the platform's recording
//! (it can't move between threads) and hands the meeting to the processing
//! queue when capture ends, whether the user stopped it or the window went
//! away.

use std::sync::mpsc::{RecvTimeoutError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use sakshi_app::{ProcessOptions, create_meeting};
use sakshi_capture::{Capture, CaptureConfig, PermissionState, Recording, SourceId, WindowView};
use sakshi_core::DataDir;
use sakshi_media::Ffmpeg;
use serde::{Deserialize, Serialize};

use crate::events::{AppEvent, Notify};
use crate::jobs::Jobs;

/// Makes the platform's capture backend. Called on the recording thread.
pub type CaptureFactory = dyn Fn() -> Box<dyn Capture> + Send + Sync;

/// How often the recording thread checks whether capture ended by itself
/// or its video paused.
const POLL: Duration = Duration::from_millis(250);

/// At most one notification about paused video in this long, so a window
/// going in and out of view doesn't flood the user.
const ALERT_GAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecorderStatus {
    Idle {
        /// Why the last recording ended, if it wasn't stopped normally.
        notice: Option<String>,
    },
    Starting,
    Recording {
        meeting_id: String,
        title: String,
        /// What's being recorded, e.g. "Zoom: Zoom Meeting".
        source: String,
        /// Unix time in milliseconds.
        started_at_ms: u64,
        microphone: bool,
        system_audio: bool,
        /// The app being recorded, for messages ("Zoom").
        app: String,
        /// Set while the window's video is paused because it is out of
        /// view (`covered` or `hidden`). Audio carries on regardless.
        video_paused: Option<WindowView>,
    },
    Stopping {
        meeting_id: String,
        title: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub source: SourceId,
    pub title: Option<String>,
    pub microphone: bool,
    pub system_audio: bool,
}

pub struct Recorder {
    data: DataDir,
    capture: Arc<CaptureFactory>,
    jobs: Arc<Jobs>,
    notify: Arc<dyn Notify>,
    state: Arc<Mutex<State>>,
}

struct State {
    status: RecorderStatus,
    stop: Option<std::sync::mpsc::Sender<()>>,
    /// What is being recorded.
    source: Option<SourceId>,
}

impl Recorder {
    pub fn new(
        data: DataDir,
        capture: Arc<CaptureFactory>,
        jobs: Arc<Jobs>,
        notify: Arc<dyn Notify>,
    ) -> Self {
        Self {
            data,
            capture,
            jobs,
            notify,
            state: Arc::new(Mutex::new(State {
                status: RecorderStatus::Idle { notice: None },
                stop: None,
                source: None,
            })),
        }
    }

    pub fn status(&self) -> RecorderStatus {
        self.state.lock().expect("recorder state").status.clone()
    }

    /// Starts recording and returns once capture is running (or failed to
    /// start, in which case nothing is left behind).
    pub fn start(&self, request: StartRequest, ffmpeg: Ffmpeg) -> anyhow::Result<RecorderStatus> {
        {
            let mut state = self.state.lock().expect("recorder state");
            if !matches!(state.status, RecorderStatus::Idle { .. }) {
                bail!("Sakshi is already recording");
            }
            state.status = RecorderStatus::Starting;
        }
        self.notify.notify(AppEvent::Recorder {
            status: RecorderStatus::Starting,
        });

        let (ready_tx, ready_rx) = sync_channel(1);
        let (stop_tx, stop_rx) = channel();
        let data = self.data.clone();
        let factory = Arc::clone(&self.capture);
        let jobs = Arc::clone(&self.jobs);
        let notify = Arc::clone(&self.notify);
        let state = Arc::clone(&self.state);
        let source = request.source;
        let spawned = std::thread::Builder::new()
            .name("sakshi-recording".into())
            .spawn(move || {
                let capture = factory();
                let started = begin(capture.as_ref(), &data, request, &ffmpeg);
                let (recording, status, folder, title) = match started {
                    Ok(started) => started,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                {
                    let mut state = state.lock().expect("recorder state");
                    state.status = status.clone();
                    state.stop = Some(stop_tx);
                    state.source = Some(source);
                }
                let _ = ready_tx.send(Ok(status.clone()));

                let app = match &status {
                    RecorderStatus::Recording { app, .. } => app.clone(),
                    _ => String::new(),
                };
                let mut paused = None;
                let mut alerted: Option<Instant> = None;
                let interruption = loop {
                    match stop_rx.recv_timeout(POLL) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break None,
                        Err(RecvTimeoutError::Timeout) => {
                            if let Some(why) = recording.interruption() {
                                break Some(why);
                            }
                            let now_paused = recording.video_paused();
                            if now_paused != paused {
                                paused = now_paused;
                                set_video_paused(&state, notify.as_ref(), paused);
                                if let Some(view) = paused
                                    && alerted.is_none_or(|at| at.elapsed() >= ALERT_GAP)
                                {
                                    let (title, body) = paused_alert(&app, view);
                                    notify.alert(&title, &body);
                                    alerted = Some(Instant::now());
                                }
                            }
                        }
                    }
                };
                let meeting_id = match &status {
                    RecorderStatus::Recording { meeting_id, .. } => meeting_id.clone(),
                    _ => String::new(),
                };
                set_status(
                    &state,
                    notify.as_ref(),
                    RecorderStatus::Stopping {
                        meeting_id: meeting_id.clone(),
                        title: title.clone(),
                    },
                );
                let notice = match (recording.stop(), interruption) {
                    (Err(e), _) => Some(format!("The recording stopped with an error: {e}")),
                    (Ok(_), Some(why)) => Some(format!("The recording ended early: {why}")),
                    (Ok(_), None) => None,
                };
                // Process whatever was captured; if nothing usable was, the
                // job says so.
                jobs.enqueue(
                    meeting_id,
                    folder,
                    title,
                    ProcessOptions::default(),
                    notify.as_ref(),
                );
                let mut guard = state.lock().expect("recorder state");
                guard.stop = None;
                guard.source = None;
                guard.status = RecorderStatus::Idle { notice };
                let status = guard.status.clone();
                drop(guard);
                notify.notify(AppEvent::Recorder { status });
            });
        if let Err(e) = spawned {
            self.reset();
            return Err(anyhow!(e).context("starting the recording thread"));
        }

        match ready_rx.recv() {
            Ok(Ok(status)) => {
                self.notify.notify(AppEvent::Recorder {
                    status: status.clone(),
                });
                Ok(status)
            }
            Ok(Err(e)) => {
                self.reset();
                Err(e)
            }
            Err(_) => {
                self.reset();
                bail!("the recording thread stopped unexpectedly")
            }
        }
    }

    /// Asks the recording to stop. Processing starts once capture has
    /// closed its files.
    pub fn stop(&self) -> anyhow::Result<()> {
        let state = self.state.lock().expect("recorder state");
        match &state.stop {
            Some(stop) => {
                // The thread may have just stopped on its own; that's fine.
                let _ = stop.send(());
                Ok(())
            }
            None => bail!("Sakshi isn't recording"),
        }
    }

    /// Brings the recorded window's app to the front, so its video
    /// resumes. Only works while Sakshi is the active app, e.g. right after
    /// a click in it.
    pub fn show(&self) -> anyhow::Result<()> {
        let source = self.state.lock().expect("recorder state").source;
        let source = source.context("Sakshi isn't recording")?;
        (self.capture)().show(source)?;
        Ok(())
    }

    fn reset(&self) {
        set_status(
            &self.state,
            self.notify.as_ref(),
            RecorderStatus::Idle { notice: None },
        );
    }
}

fn set_status(state: &Mutex<State>, notify: &dyn Notify, status: RecorderStatus) {
    state.lock().expect("recorder state").status = status.clone();
    notify.notify(AppEvent::Recorder { status });
}

fn set_video_paused(state: &Mutex<State>, notify: &dyn Notify, paused: Option<WindowView>) {
    let status = {
        let mut state = state.lock().expect("recorder state");
        if let RecorderStatus::Recording { video_paused, .. } = &mut state.status {
            *video_paused = paused;
        }
        state.status.clone()
    };
    notify.notify(AppEvent::Recorder { status });
}

/// The notification's title and body when `app`'s video pauses.
fn paused_alert(app: &str, view: WindowView) -> (String, String) {
    let (where_, fix) = match view {
        WindowView::Covered => (
            "is behind other windows",
            "Leave any part of it showing to keep its video.",
        ),
        _ => (
            "is minimized or on another desktop",
            "Bring it back to this desktop to keep its video.",
        ),
    };
    (
        "Video paused".into(),
        format!("{app}'s window {where_}, so it stopped updating. Audio is still recording. {fix}"),
    )
}

type Started = (
    Box<dyn Recording>,
    RecorderStatus,
    std::path::PathBuf,
    String,
);

/// Checks access, creates the meeting folder, and starts capture. The
/// folder is removed again if capture doesn't start.
fn begin(
    capture: &dyn Capture,
    data: &DataDir,
    request: StartRequest,
    ffmpeg: &Ffmpeg,
) -> anyhow::Result<Started> {
    let permissions = capture.permissions();
    if permissions.screen != PermissionState::Granted {
        bail!("Sakshi needs Screen Recording access to record a window");
    }
    if request.microphone && permissions.microphone != PermissionState::Granted {
        bail!("Sakshi needs Microphone access to record your side of the call");
    }
    let source = capture
        .sources()?
        .into_iter()
        .find(|s| s.id == request.source)
        .context("that window or display is no longer available; pick it again")?;
    let (meeting, info) = create_meeting(data, &source, request.title)?;
    let config = CaptureConfig {
        microphone: request.microphone,
        system_audio: request.system_audio,
        ..CaptureConfig::default()
    };
    let recording = match capture.start(source.id, &config, &meeting, ffmpeg) {
        Ok(recording) => recording,
        Err(e) => {
            // Only meeting.json and an empty capture folder exist yet.
            let _ = std::fs::remove_dir_all(meeting.path());
            return Err(e.into());
        }
    };
    let title = info.title.unwrap_or_default();
    let app = source
        .meeting
        .clone()
        .or_else(|| source.app_name.clone().filter(|a| !a.is_empty()));
    let label = match &app {
        Some(app) => format!("{app}: {}", source.title),
        None => source.title.clone(),
    };
    let status = RecorderStatus::Recording {
        meeting_id: info.id.unwrap_or_default(),
        title: title.clone(),
        source: label,
        started_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        microphone: request.microphone,
        system_audio: request.system_audio,
        app: app.unwrap_or_else(|| "The call".into()),
        video_paused: None,
    };
    Ok((recording, status, meeting.path().to_path_buf(), title))
}

#[cfg(test)]
pub mod testing {
    //! A capture backend that records nothing, for tests.

    use std::time::Instant;

    use sakshi_capture::{CaptureError, CaptureStats, Permissions, Source};
    use sakshi_core::MeetingDir;

    use super::*;

    #[derive(Default)]
    pub struct FakeControl {
        pub fail_start: bool,
        pub deny_microphone: bool,
        pub interruption: Mutex<Option<String>>,
        pub video_paused: Mutex<Option<WindowView>>,
        pub shown: Mutex<Vec<SourceId>>,
        pub stopped: Mutex<u32>,
    }

    pub struct FakeCapture(pub Arc<FakeControl>);

    pub fn window() -> Source {
        Source {
            id: SourceId::Window(42),
            title: "Zoom Meeting".into(),
            app_name: Some("zoom.us".into()),
            bundle_id: Some("us.zoom.xos".into()),
            width: 1280,
            height: 720,
            meeting: Some("Zoom".into()),
            on_screen: true,
        }
    }

    impl Capture for FakeCapture {
        fn permissions(&self) -> Permissions {
            Permissions {
                screen: PermissionState::Granted,
                microphone: if self.0.deny_microphone {
                    PermissionState::Denied
                } else {
                    PermissionState::Granted
                },
            }
        }

        fn request_screen_permission(&self) -> bool {
            true
        }

        fn request_microphone_permission(&self) -> PermissionState {
            self.permissions().microphone
        }

        fn sources(&self) -> Result<Vec<Source>, CaptureError> {
            Ok(vec![window()])
        }

        fn start(
            &self,
            _source: SourceId,
            _config: &CaptureConfig,
            meeting: &MeetingDir,
            _ffmpeg: &Ffmpeg,
        ) -> Result<Box<dyn Recording>, CaptureError> {
            std::fs::create_dir_all(sakshi_capture::capture_dir(meeting)).unwrap();
            if self.0.fail_start {
                return Err(CaptureError::Unsupported("the fake was told to fail"));
            }
            Ok(Box::new(FakeRecording {
                started: Instant::now(),
                control: Arc::clone(&self.0),
            }))
        }

        fn show(&self, source: SourceId) -> Result<(), CaptureError> {
            self.0.shown.lock().unwrap().push(source);
            Ok(())
        }
    }

    struct FakeRecording {
        started: Instant,
        control: Arc<FakeControl>,
    }

    impl Recording for FakeRecording {
        fn elapsed(&self) -> Duration {
            self.started.elapsed()
        }

        fn interruption(&self) -> Option<String> {
            self.control.interruption.lock().unwrap().clone()
        }

        fn video_paused(&self) -> Option<WindowView> {
            *self.control.video_paused.lock().unwrap()
        }

        fn stop(self: Box<Self>) -> Result<CaptureStats, CaptureError> {
            *self.control.stopped.lock().unwrap() += 1;
            Ok(CaptureStats::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sakshi_core::{MeetingInfo, read_json};

    use super::testing::*;
    use super::*;
    use crate::events::testing::Recorded;
    use crate::jobs::{JobState, Processor};
    use crate::test_util::wait_for;

    struct Rig {
        _root: tempfile::TempDir,
        data: DataDir,
        control: Arc<FakeControl>,
        notify: Arc<Recorded>,
        jobs: Arc<Jobs>,
        recorder: Recorder,
    }

    fn rig(control: FakeControl) -> Rig {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path());
        let control = Arc::new(control);
        let notify = Arc::new(Recorded::default());
        let processor: Arc<Processor> = Arc::new(|folder: &Path, _, _| {
            assert!(folder.join("meeting.json").is_file());
            Ok("done".into())
        });
        let jobs = Arc::new(Jobs::start(processor, notify.clone()));
        let factory_control = Arc::clone(&control);
        let factory: Arc<CaptureFactory> =
            Arc::new(move || Box::new(FakeCapture(Arc::clone(&factory_control))));
        let recorder = Recorder::new(data.clone(), factory, Arc::clone(&jobs), notify.clone());
        Rig {
            _root: root,
            data,
            control,
            notify,
            jobs,
            recorder,
        }
    }

    fn request() -> StartRequest {
        StartRequest {
            source: SourceId::Window(42),
            title: None,
            microphone: true,
            system_audio: true,
        }
    }

    fn ffmpeg() -> Ffmpeg {
        Ffmpeg::new("unused-by-the-fake")
    }

    fn meeting_id(status: &RecorderStatus) -> String {
        match status {
            RecorderStatus::Recording { meeting_id, .. } => meeting_id.clone(),
            other => panic!("not recording: {other:?}"),
        }
    }

    #[test]
    fn records_then_queues_processing() {
        let rig = rig(FakeControl::default());

        let status = rig.recorder.start(request(), ffmpeg()).unwrap();
        let id = meeting_id(&status);
        let RecorderStatus::Recording { title, source, .. } = &status else {
            unreachable!()
        };
        assert_eq!(title, "Zoom Meeting");
        assert_eq!(source, "Zoom: Zoom Meeting");
        let info: MeetingInfo = read_json(&rig.data.meeting(&id).info_json()).unwrap();
        assert_eq!(info.id.as_deref(), Some(id.as_str()));
        assert!(
            rig.recorder.start(request(), ffmpeg()).is_err(),
            "a second start"
        );

        rig.recorder.stop().unwrap();
        wait_for("processing to finish", || {
            rig.jobs
                .for_meeting(&id)
                .is_some_and(|j| j.state == JobState::Done)
        });
        wait_for("the recorder to be idle", || {
            rig.recorder.status() == RecorderStatus::Idle { notice: None }
        });
        assert_eq!(*rig.control.stopped.lock().unwrap(), 1);
        let states: Vec<RecorderStatus> = rig
            .notify
            .take()
            .into_iter()
            .filter_map(|e| match e {
                AppEvent::Recorder { status } => Some(status),
                _ => None,
            })
            .collect();
        assert!(matches!(states[0], RecorderStatus::Starting));
        assert!(
            states
                .iter()
                .any(|s| matches!(s, RecorderStatus::Stopping { .. }))
        );
        assert_eq!(states.last(), Some(&RecorderStatus::Idle { notice: None }));
        assert!(rig.recorder.stop().is_err());
    }

    #[test]
    fn capture_that_ends_by_itself_is_processed_with_a_notice() {
        let rig = rig(FakeControl::default());
        let id = meeting_id(&rig.recorder.start(request(), ffmpeg()).unwrap());

        *rig.control.interruption.lock().unwrap() = Some("the window closed".into());

        wait_for("the recorder to notice", || {
            matches!(
                rig.recorder.status(),
                RecorderStatus::Idle { notice: Some(_) }
            )
        });
        assert_eq!(
            rig.recorder.status(),
            RecorderStatus::Idle {
                notice: Some("The recording ended early: the window closed".into())
            }
        );
        wait_for("the job", || rig.jobs.for_meeting(&id).is_some());
    }

    fn video_paused(status: &RecorderStatus) -> Option<WindowView> {
        match status {
            RecorderStatus::Recording { video_paused, .. } => *video_paused,
            other => panic!("not recording: {other:?}"),
        }
    }

    #[test]
    fn paused_video_is_reported_and_alerted_once_a_minute() {
        let rig = rig(FakeControl::default());
        rig.recorder.start(request(), ffmpeg()).unwrap();

        *rig.control.video_paused.lock().unwrap() = Some(WindowView::Hidden);
        wait_for("the pause", || {
            video_paused(&rig.recorder.status()) == Some(WindowView::Hidden)
        });
        let alerts = rig.notify.take_alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].0, "Video paused");
        assert!(
            alerts[0]
                .1
                .starts_with("Zoom's window is minimized or on another desktop"),
            "{}",
            alerts[0].1
        );
        assert!(alerts[0].1.contains("Audio is still recording"));

        *rig.control.video_paused.lock().unwrap() = None;
        wait_for("the resume", || {
            video_paused(&rig.recorder.status()).is_none()
        });
        // Paused again soon after: the UI shows it, but no second alert.
        *rig.control.video_paused.lock().unwrap() = Some(WindowView::Covered);
        wait_for("the second pause", || {
            video_paused(&rig.recorder.status()) == Some(WindowView::Covered)
        });
        assert!(rig.notify.take_alerts().is_empty());
        let paused_events = rig
            .notify
            .take()
            .into_iter()
            .filter(|e| {
                matches!(
                    e,
                    AppEvent::Recorder {
                        status: RecorderStatus::Recording {
                            video_paused: Some(_),
                            ..
                        }
                    }
                )
            })
            .count();
        assert_eq!(paused_events, 2);

        rig.recorder.show().unwrap();
        assert_eq!(*rig.control.shown.lock().unwrap(), [SourceId::Window(42)]);
        rig.recorder.stop().unwrap();
        wait_for("the recorder to be idle", || {
            matches!(rig.recorder.status(), RecorderStatus::Idle { .. })
        });
        assert!(rig.recorder.show().is_err(), "nothing to show when idle");
    }

    #[test]
    fn a_failed_start_leaves_nothing_behind() {
        let rig = rig(FakeControl {
            fail_start: true,
            ..FakeControl::default()
        });

        assert!(rig.recorder.start(request(), ffmpeg()).is_err());

        assert_eq!(rig.recorder.status(), RecorderStatus::Idle { notice: None });
        let leftovers = std::fs::read_dir(rig.data.meetings_dir())
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0);
        assert!(rig.jobs.list().is_empty());
    }

    #[test]
    fn checks_access_before_creating_anything() {
        let rig = rig(FakeControl {
            deny_microphone: true,
            ..FakeControl::default()
        });

        let err = rig.recorder.start(request(), ffmpeg()).unwrap_err();
        assert!(err.to_string().contains("Microphone"), "{err}");
        assert!(!rig.data.meetings_dir().exists());

        let without_mic = StartRequest {
            microphone: false,
            ..request()
        };
        rig.recorder.start(without_mic, ffmpeg()).unwrap();
        rig.recorder.stop().unwrap();
    }

    #[test]
    fn unknown_sources_are_refused() {
        let rig = rig(FakeControl::default());
        let request = StartRequest {
            source: SourceId::Display(7),
            ..request()
        };
        let err = rig.recorder.start(request, ffmpeg()).unwrap_err();
        assert!(err.to_string().contains("no longer available"), "{err}");
    }
}
