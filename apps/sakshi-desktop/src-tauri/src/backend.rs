//! Everything the UI can ask for, as plain methods so tests can call them
//! without a window. `main.rs` exposes them as Tauri commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use sakshi_app::mcp_setup::{
    self, Client, ClientPaths, ClientSetup, InstallOutcome, ServerLaunch, locate_mcp_server,
};
use sakshi_app::{ProcessOptions, check_language, load_settings, save_settings};
use sakshi_capture::{PermissionState, Source};
use sakshi_core::{
    DataDir, ImportRequest, ImportState, Language, MeetingDir, MeetingInfo, Speaker, SpeakerNames,
    Summary, Track, read_json,
};
use sakshi_media::{Ffmpeg, MIN_FFMPEG};
use sakshi_models::{ModelStore, Role};
use sakshi_store::{MeetingQuery, MeetingRecord, Store, TimeRange};
use serde::{Deserialize, Serialize};

use crate::events::{AppEvent, Notify};
use crate::jobs::{JobState, JobView, Jobs, Processor};
use crate::models::Models;
use crate::recorder::{CaptureFactory, Recorder, RecorderStatus, StartRequest};

/// Most meetings the list shows.
const MEETING_LIMIT: u32 = 500;

pub struct Backend {
    pub data: DataDir,
    capture: Arc<CaptureFactory>,
    pub recorder: Recorder,
    pub jobs: Arc<Jobs>,
    pub models: Models,
    notify: Arc<dyn Notify>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Screen,
    Microphone,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionsView {
    /// Whether this OS can record yet (macOS today).
    pub supported: bool,
    pub screen: PermissionState,
    pub microphone: PermissionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Recording,
    Queued,
    Processing,
    Failed,
    /// Recorded, never processed (or interrupted before it could be).
    Unprocessed,
    Processed,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingItem {
    pub id: String,
    pub title: String,
    pub started_at: Option<String>,
    pub duration_ms: Option<u64>,
    /// Labels, in order of first appearance.
    pub speakers: Vec<String>,
    /// Names the user gave some of those labels.
    pub speaker_names: SpeakerNames,
    /// The file a recording made elsewhere came from.
    pub imported_from: Option<String>,
    pub segment_count: u32,
    pub frame_count: u32,
    pub status: MeetingStatus,
    pub job: Option<JobView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentView {
    pub id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub track: Track,
    pub speaker: Speaker,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameView {
    pub at_ms: u64,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingDetail {
    #[serde(flatten)]
    pub item: MeetingItem,
    pub folder: PathBuf,
    pub recording: Option<PathBuf>,
    /// The language pinned for this meeting, if any.
    pub language: Option<Language>,
    /// Languages the engine reported.
    pub languages: Vec<String>,
    pub segments: Vec<SegmentView>,
    pub frames: Vec<FrameView>,
    pub summaries: Vec<Summary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub language: Language,
    pub consent_reminder: bool,
    pub data_dir: PathBuf,
    /// Codes the default ASR model accepts, once it's downloaded.
    pub supported_languages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub ok: bool,
    pub detail: String,
}

impl Check {
    fn from<T>(result: anyhow::Result<T>, ok: impl FnOnce(T) -> String) -> Self {
        match result {
            Ok(value) => Self {
                ok: true,
                detail: ok(value),
            },
            Err(e) => Self {
                ok: false,
                detail: format!("{e:#}"),
            },
        }
    }
}

/// Whether recordings can be processed on this machine.
#[derive(Debug, Clone, Serialize)]
pub struct SetupCheck {
    pub ffmpeg: Check,
    pub transcriber: Check,
    pub models: Check,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpView {
    pub server: Option<PathBuf>,
    pub error: Option<String>,
    pub clients: Vec<ClientSetup>,
}

impl Backend {
    pub fn new(
        data: DataDir,
        capture: Arc<CaptureFactory>,
        processor: Arc<Processor>,
        notify: Arc<dyn Notify>,
    ) -> Self {
        let jobs = Arc::new(Jobs::start(processor, Arc::clone(&notify)));
        Self {
            recorder: Recorder::new(
                data.clone(),
                Arc::clone(&capture),
                Arc::clone(&jobs),
                Arc::clone(&notify),
            ),
            models: Models::new(data.clone(), Arc::clone(&notify)),
            data,
            capture,
            jobs,
            notify,
        }
    }

    pub fn permissions(&self) -> PermissionsView {
        let permissions = (self.capture)().permissions();
        PermissionsView {
            supported: cfg!(target_os = "macos"),
            screen: permissions.screen,
            microphone: permissions.microphone,
        }
    }

    /// Shows the system prompt where the OS still allows one.
    pub fn request_permission(&self, kind: PermissionKind) -> PermissionsView {
        let capture = (self.capture)();
        match kind {
            PermissionKind::Screen => {
                capture.request_screen_permission();
            }
            PermissionKind::Microphone => {
                capture.request_microphone_permission();
            }
        }
        self.permissions()
    }

    pub fn sources(&self) -> anyhow::Result<Vec<Source>> {
        Ok((self.capture)().sources()?)
    }

    pub fn start_recording(&self, request: StartRequest) -> anyhow::Result<RecorderStatus> {
        self.recorder.start(request, Ffmpeg::locate()?)
    }

    /// Meetings, newest first: everything in the index plus recordings that
    /// haven't been processed yet. `query` matches titles and transcripts.
    pub fn meetings(&self, query: Option<&str>) -> anyhow::Result<Vec<MeetingItem>> {
        let query = query.map(str::trim).filter(|q| !q.is_empty());
        let store = Store::open(&self.data.db_path())?;
        let records = store.meetings(&MeetingQuery {
            text: query.map(str::to_owned),
            limit: MEETING_LIMIT,
            ..MeetingQuery::default()
        })?;
        let indexed: HashSet<PathBuf> = store
            .meetings(&MeetingQuery {
                limit: u32::MAX,
                ..MeetingQuery::default()
            })?
            .into_iter()
            .map(|r| r.folder)
            .collect();

        let mut items: Vec<MeetingItem> = records.into_iter().map(|r| self.item(r)).collect();
        for (id, info) in self.unindexed_folders(&indexed)? {
            let item = self.unprocessed_item(id, info);
            if query.is_none_or(|q| item.title.to_lowercase().contains(&q.to_lowercase())) {
                items.push(item);
            }
        }
        items.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
        Ok(items)
    }

    /// Folders under `meetings/` that the index doesn't know.
    fn unindexed_folders(
        &self,
        indexed: &HashSet<PathBuf>,
    ) -> anyhow::Result<Vec<(String, MeetingInfo)>> {
        let dir = self.data.meetings_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
        };
        let mut found = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let folder = entry.path();
            let meeting = MeetingDir::new(&folder);
            if !meeting.info_json().is_file() || indexed.contains(&absolute(&folder)) {
                continue;
            }
            let Ok(info) = read_json::<MeetingInfo>(&meeting.info_json()) else {
                continue;
            };
            found.push((entry.file_name().to_string_lossy().into_owned(), info));
        }
        Ok(found)
    }

    fn item(&self, record: MeetingRecord) -> MeetingItem {
        let job = self.jobs.for_meeting(&record.id);
        let status = self.status(&record.id, job.as_ref(), true);
        MeetingItem {
            id: record.id,
            title: record.title,
            started_at: record.started_at,
            duration_ms: Some(record.duration_ms),
            speakers: record.speakers,
            speaker_names: record.speaker_names,
            imported_from: record.imported_from,
            segment_count: record.segment_count,
            frame_count: record.frame_count,
            status,
            job,
        }
    }

    fn unprocessed_item(&self, id: String, info: MeetingInfo) -> MeetingItem {
        let job = self.jobs.for_meeting(&id);
        let mut status = self.status(&id, job.as_ref(), false);
        // An import that failed before this run of the app has no job.
        if status == MeetingStatus::Unprocessed
            && let Ok(Some(request)) = ImportRequest::load(&self.data, &id)
            && matches!(request.state, ImportState::Failed { .. })
        {
            status = MeetingStatus::Failed;
        }
        MeetingItem {
            status,
            id,
            title: info.title.unwrap_or_else(|| "Recording".into()),
            started_at: info.started_at,
            duration_ms: None,
            speakers: Vec::new(),
            speaker_names: SpeakerNames::default(),
            imported_from: info.imported_from,
            segment_count: 0,
            frame_count: 0,
            job,
        }
    }

    fn status(&self, id: &str, job: Option<&JobView>, processed: bool) -> MeetingStatus {
        let recording = match self.recorder.status() {
            RecorderStatus::Recording { meeting_id, .. }
            | RecorderStatus::Stopping { meeting_id, .. } => meeting_id == id,
            _ => false,
        };
        match (recording, job.map(|j| &j.state)) {
            (true, _) => MeetingStatus::Recording,
            (_, Some(JobState::Queued)) => MeetingStatus::Queued,
            (_, Some(JobState::Running)) => MeetingStatus::Processing,
            _ if processed => MeetingStatus::Processed,
            (_, Some(JobState::Failed { .. })) => MeetingStatus::Failed,
            _ => MeetingStatus::Unprocessed,
        }
    }

    /// The meeting's folder: from the index, else `meetings/<id>/`.
    pub fn meeting_folder(&self, id: &str) -> anyhow::Result<PathBuf> {
        let store = Store::open(&self.data.db_path())?;
        if let Some(record) = store.meeting(id)? {
            return Ok(record.folder);
        }
        check_id(id)?;
        let meeting = self.data.meeting(id);
        if !meeting.info_json().is_file() {
            bail!("no meeting {id}");
        }
        Ok(meeting.path().to_path_buf())
    }

    pub fn meeting(&self, id: &str) -> anyhow::Result<MeetingDetail> {
        let folder = self.meeting_folder(id)?;
        let dir = MeetingDir::new(&folder);
        let info: MeetingInfo = read_json(&dir.info_json()).unwrap_or_default();
        let recording = Some(dir.recording_mp4()).filter(|p| p.is_file());
        let store = Store::open(&self.data.db_path())?;
        let Some(record) = store.meeting(id)? else {
            let language = info.language.clone();
            return Ok(MeetingDetail {
                item: self.unprocessed_item(id.to_owned(), info),
                folder,
                recording,
                language,
                languages: Vec::new(),
                segments: Vec::new(),
                frames: Vec::new(),
                summaries: Vec::new(),
            });
        };
        let segments = store
            .segments(id, TimeRange::ALL, None, u32::MAX)?
            .into_iter()
            .map(|s| SegmentView {
                id: s.id,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                track: s.track,
                speaker: s.speaker,
                text: s.text,
            })
            .collect();
        let frames = store
            .frames(id, TimeRange::ALL)?
            .into_iter()
            .map(|f| FrameView {
                at_ms: f.at_ms,
                path: folder.join(&f.file),
                width: f.width,
                height: f.height,
            })
            .collect();
        let summaries = store.summaries(id)?;
        let languages = record.languages.clone();
        Ok(MeetingDetail {
            item: self.item(record),
            folder,
            recording,
            language: info.language,
            languages,
            segments,
            frames,
            summaries,
        })
    }

    /// Queues a meeting for processing, optionally with a different
    /// language (pinned for later runs too) or from scratch.
    pub fn process(
        &self,
        id: &str,
        language: Option<Language>,
        force: bool,
    ) -> anyhow::Result<JobView> {
        let folder = self.meeting_folder(id)?;
        if let Some(language) = &language {
            check_language(&self.data, language)?;
            sakshi_app::set_meeting_language(&MeetingDir::new(&folder), Some(language.clone()))?;
        }
        let title = read_json::<MeetingInfo>(&MeetingDir::new(&folder).info_json())
            .ok()
            .and_then(|i| i.title)
            .unwrap_or_else(|| "Recording".into());
        let options = ProcessOptions {
            force,
            ..ProcessOptions::default()
        };
        Ok(self
            .jobs
            .enqueue(id.to_owned(), folder, title, options, self.notify.as_ref()))
    }

    /// Pins a meeting's language, or with `None` follows the global setting.
    pub fn set_meeting_language(&self, id: &str, language: Option<Language>) -> anyhow::Result<()> {
        if let Some(language) = &language {
            check_language(&self.data, language)?;
        }
        let folder = self.meeting_folder(id)?;
        Ok(sakshi_app::set_meeting_language(
            &MeetingDir::new(folder),
            language,
        )?)
    }

    /// Names (or, with a blank name, un-names) speakers by label.
    pub fn set_speaker_names(
        &self,
        id: &str,
        names: Vec<(String, Option<String>)>,
    ) -> anyhow::Result<SpeakerNames> {
        let names = Store::open(&self.data.db_path())?.set_speaker_names(id, &names)?;
        self.notify.notify(AppEvent::MeetingsChanged);
        Ok(names)
    }

    /// Queues a recording made elsewhere and starts on it; returns the id
    /// its meeting will have.
    pub fn import(&self, file: &Path) -> anyhow::Result<String> {
        let request = sakshi_app::request_import(&self.data, file, None, None, None)?;
        self.take_imports()?;
        Ok(request.id)
    }

    /// Queues every import waiting in `imports/`, from this app or the MCP
    /// server. One left `processing` by an app that quit starts over; a
    /// failed one waits for the user to retry it.
    pub fn take_imports(&self) -> anyhow::Result<()> {
        let mut added = false;
        for request in ImportRequest::all(&self.data)? {
            if matches!(request.state, ImportState::Failed { .. }) {
                continue;
            }
            let meeting = self.data.meeting(&request.id);
            added |= !meeting.info_json().is_file();
            sakshi_app::import_folder(&self.data, &request)?;
            let title = read_json::<MeetingInfo>(&meeting.info_json())?
                .title
                .unwrap_or_else(|| request.file_name());
            self.jobs.enqueue(
                request.id.clone(),
                meeting.path().to_path_buf(),
                title,
                ProcessOptions::default(),
                self.notify.as_ref(),
            );
        }
        if added {
            self.notify.notify(AppEvent::MeetingsChanged);
        }
        Ok(())
    }

    pub fn settings(&self) -> anyhow::Result<SettingsView> {
        let settings = load_settings(&self.data)?;
        Ok(SettingsView {
            language: settings.language,
            consent_reminder: settings.consent_reminder,
            data_dir: self.data.root().to_path_buf(),
            // A broken model file shouldn't keep the settings page from opening.
            supported_languages: sakshi_app::supported_languages(&self.data).unwrap_or(None),
        })
    }

    pub fn save_settings(
        &self,
        language: Language,
        consent_reminder: bool,
    ) -> anyhow::Result<SettingsView> {
        check_language(&self.data, &language)?;
        let mut settings = load_settings(&self.data)?;
        settings.language = language;
        settings.consent_reminder = consent_reminder;
        save_settings(&self.data, &settings)?;
        self.settings()
    }

    pub fn setup_check(&self) -> SetupCheck {
        let models = || -> anyhow::Result<()> {
            let store = ModelStore::open(&self.data)?;
            let missing: Vec<String> = [Role::Asr, Role::Diarization]
                .into_iter()
                .map(|role| store.default_for(role))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|entry| matches!(store.installed_path(entry), Ok(None)))
                .map(|entry| entry.name)
                .collect();
            if !missing.is_empty() {
                bail!("download {} on the Models page", missing.join(" and "));
            }
            Ok(())
        };
        // Sakshi doesn't ship FFmpeg, so name the version it found.
        let ffmpeg = || -> anyhow::Result<String> {
            let ffmpeg = Ffmpeg::locate()?;
            let version = ffmpeg.version()?;
            let found = format!("FFmpeg {} at {}", version.text, ffmpeg.path().display());
            if !version.is_supported() {
                let (major, minor) = MIN_FFMPEG;
                bail!(
                    "{found} is older than {major}.{minor}, the oldest version Sakshi is \
                     tested with. Update it, e.g. `brew upgrade ffmpeg`."
                );
            }
            Ok(found)
        };
        SetupCheck {
            ffmpeg: Check::from(ffmpeg(), |found| found),
            transcriber: Check::from(sakshi_asr::locate_sidecar().map_err(Into::into), |p| {
                p.display().to_string()
            }),
            models: Check::from(models(), |()| "downloaded".into()),
        }
    }

    pub fn mcp(&self) -> McpView {
        let clients = |server: &Path| -> anyhow::Result<Vec<ClientSetup>> {
            let launch = ServerLaunch::new(server.to_path_buf(), &self.data);
            Ok(mcp_setup::clients(&launch, &ClientPaths::detect()?))
        };
        match locate_mcp_server().map_err(anyhow::Error::from) {
            Ok(server) => match clients(&server) {
                Ok(clients) => McpView {
                    server: Some(server),
                    error: None,
                    clients,
                },
                Err(e) => McpView {
                    server: Some(server),
                    error: Some(format!("{e:#}")),
                    clients: Vec::new(),
                },
            },
            Err(e) => McpView {
                server: None,
                error: Some(format!("{e:#}")),
                clients: Vec::new(),
            },
        }
    }

    pub fn mcp_install(&self, client: Client) -> anyhow::Result<InstallOutcome> {
        let launch = ServerLaunch::new(locate_mcp_server()?, &self.data);
        Ok(mcp_setup::install(
            client,
            &launch,
            &ClientPaths::detect()?,
        )?)
    }
}

/// Meeting ids from the UI become folder names, so only plain names pass.
fn check_id(id: &str) -> anyhow::Result<()> {
    let plain = !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if !plain {
        bail!("{id:?} isn't a meeting id");
    }
    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use sakshi_core::{Segment, Transcript, write_json_atomic};

    use super::*;
    use crate::events::testing::Recorded;
    use crate::recorder::testing::{FakeCapture, FakeControl};
    use crate::test_util::wait_for;

    fn backend(root: &Path, processor: Arc<Processor>) -> Backend {
        let control = Arc::new(FakeControl::default());
        Backend::new(
            DataDir::new(root),
            Arc::new(move || Box::new(FakeCapture(Arc::clone(&control)))),
            processor,
            Arc::new(Recorded::default()),
        )
    }

    fn never_called() -> Arc<Processor> {
        Arc::new(|_, _, _| anyhow::bail!("not in this test"))
    }

    /// A processed meeting: meeting.json, transcript.json, and the index.
    fn processed(data: &DataDir, id: &str, title: &str, started_at: &str, text: &str) {
        let dir = data.meeting(id);
        write_json_atomic(
            &dir.info_json(),
            &MeetingInfo {
                id: Some(id.into()),
                title: Some(title.into()),
                started_at: Some(started_at.into()),
                ..MeetingInfo::default()
            },
        )
        .unwrap();
        let transcript = Transcript {
            schema_version: Transcript::SCHEMA_VERSION,
            duration_ms: 5_000,
            languages: vec!["en".into()],
            segments: vec![Segment {
                id: 0,
                start_ms: 0,
                end_ms: 2_000,
                track: Track::System,
                speaker: Speaker::Remote(1),
                text: text.into(),
                words: Vec::new(),
            }],
        };
        write_json_atomic(&dir.transcript_json(), &transcript).unwrap();
        Store::open(&data.db_path())
            .unwrap()
            .index_meeting(&dir)
            .unwrap();
    }

    fn unprocessed(data: &DataDir, id: &str, title: &str, started_at: &str) {
        write_json_atomic(
            &data.meeting(id).info_json(),
            &MeetingInfo {
                id: Some(id.into()),
                title: Some(title.into()),
                started_at: Some(started_at.into()),
                ..MeetingInfo::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn names_speakers_and_shows_the_names() {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), never_called());
        processed(
            &backend.data,
            A,
            "Design review",
            "2026-10-01T10:00:00Z",
            "hi",
        );

        let names = backend
            .set_speaker_names(A, vec![("Speaker 1".into(), Some("Priya".into()))])
            .unwrap();
        assert_eq!(names.display("Speaker 1"), "Priya");
        assert_eq!(backend.meeting(A).unwrap().item.speaker_names, names);
        assert!(
            backend
                .set_speaker_names(A, vec![("Speaker 4".into(), Some("Ravi".into()))])
                .is_err()
        );
    }

    #[test]
    fn takes_up_imports_and_lists_them_while_they_wait() {
        let root = tempfile::tempdir().unwrap();
        let processed_ids = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&processed_ids);
        let processor: Arc<Processor> = Arc::new(move |folder, _, _| {
            let id = folder.file_name().unwrap().to_string_lossy().into_owned();
            seen.lock().unwrap().push(id.clone());
            Ok(id)
        });
        let backend = backend(root.path(), processor);
        let file = root.path().join("Zoom call.mp4");
        std::fs::write(&file, b"media").unwrap();

        // Queued by the MCP server: picked up, given a folder, processed.
        let request = sakshi_app::request_import(&backend.data, &file, None, None, None).unwrap();
        backend.take_imports().unwrap();
        let item = &backend.meetings(None).unwrap()[0];
        assert_eq!(item.id, request.id);
        assert_eq!(item.title, "Zoom call");
        assert_eq!(item.imported_from.as_deref(), Some("Zoom call.mp4"));
        wait_for("the import job", || {
            processed_ids.lock().unwrap().as_slice() == [request.id.clone()]
        });

        // A failed import isn't retried by itself, and shows as failed.
        let failed = sakshi_app::request_import(&backend.data, &file, None, None, None).unwrap();
        ImportRequest {
            state: ImportState::Failed {
                error: "no audio".into(),
            },
            ..failed.clone()
        }
        .save(&backend.data)
        .unwrap();
        sakshi_app::import_folder(&backend.data, &failed).unwrap();
        backend.take_imports().unwrap();
        assert_eq!(processed_ids.lock().unwrap().len(), 1);
        assert_eq!(
            backend.meeting(&failed.id).unwrap().item.status,
            MeetingStatus::Failed
        );
    }

    const A: &str = "019a0000-0000-7000-8000-00000000000a";
    const B: &str = "019a0000-0000-7000-8000-00000000000b";

    #[test]
    fn lists_processed_and_unprocessed_meetings_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), never_called());
        processed(
            &backend.data,
            A,
            "Design review",
            "2026-10-01T10:00:00Z",
            "ship the beta",
        );
        unprocessed(&backend.data, B, "Standup", "2026-10-02T09:00:00Z");

        let items = backend.meetings(None).unwrap();

        let summary: Vec<(&str, MeetingStatus)> =
            items.iter().map(|m| (m.title.as_str(), m.status)).collect();
        assert_eq!(
            summary,
            [
                ("Standup", MeetingStatus::Unprocessed),
                ("Design review", MeetingStatus::Processed)
            ]
        );
        assert_eq!(items[1].segment_count, 1);

        let found = backend.meetings(Some("beta")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, A);
        assert_eq!(backend.meetings(Some("stand")).unwrap()[0].id, B);
    }

    #[test]
    fn shows_a_meeting_with_its_transcript() {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), never_called());
        processed(
            &backend.data,
            A,
            "Design review",
            "2026-10-01T10:00:00Z",
            "ship it",
        );
        unprocessed(&backend.data, B, "Standup", "2026-10-02T09:00:00Z");

        let detail = backend.meeting(A).unwrap();
        assert_eq!(detail.segments.len(), 1);
        assert_eq!(detail.segments[0].text, "ship it");
        assert!(detail.recording.is_none());

        let detail = backend.meeting(B).unwrap();
        assert_eq!(detail.item.status, MeetingStatus::Unprocessed);
        assert!(detail.segments.is_empty());

        assert!(backend.meeting("../outside").is_err());
        assert!(backend.meeting("missing").is_err());
    }

    #[test]
    fn processing_a_meeting_queues_it_and_pins_its_language() {
        let root = tempfile::tempdir().unwrap();
        let processor: Arc<Processor> = Arc::new(|folder, _, _| {
            assert!(folder.ends_with(B));
            Ok(B.into())
        });
        let backend = backend(root.path(), processor);
        unprocessed(&backend.data, B, "Standup", "2026-10-02T09:00:00Z");

        let job = backend
            .process(B, Some(Language::Code("hi".into())), false)
            .unwrap();
        assert_eq!(job.title, "Standup");
        wait_for("the job", || {
            backend
                .jobs
                .for_meeting(B)
                .is_some_and(|j| j.state == JobState::Done)
        });
        let info: MeetingInfo = read_json(&backend.data.meeting(B).info_json()).unwrap();
        assert_eq!(info.language, Some(Language::Code("hi".into())));
    }

    #[test]
    fn saves_settings() {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), never_called());

        let before = backend.settings().unwrap();
        assert_eq!(before.language, Language::Auto);
        assert!(before.consent_reminder);
        assert!(before.supported_languages.is_none());

        let after = backend
            .save_settings(Language::Code("en".into()), false)
            .unwrap();
        assert_eq!(after.language, Language::Code("en".into()));
        assert!(!after.consent_reminder);
        assert_eq!(
            load_settings(&backend.data).unwrap().language,
            after.language
        );
    }

    #[test]
    fn the_setup_check_names_missing_models() {
        let root = tempfile::tempdir().unwrap();
        let backend = backend(root.path(), never_called());
        let check = backend.setup_check();
        assert!(!check.models.ok);
        assert!(
            check.models.detail.contains("Models page"),
            "{}",
            check.models.detail
        );
    }

    #[test]
    fn ids_must_be_plain_names() {
        assert!(check_id(A).is_ok());
        for bad in ["", "..", ".hidden", "a/b", r"a\b", "a b"] {
            assert!(check_id(bad).is_err(), "{bad:?}");
        }
    }

    /// Records a real window for five seconds through the app's own
    /// recording thread, then lets the processing queue finish it, as
    /// pressing Record and Stop does. Without the microphone.
    ///
    /// Needs Screen Recording access for the terminal, ffmpeg, the
    /// nemo-speech sidecar, and models in `$SAKSHI_DATA_DIR` (default
    /// `target/dev-data`). Records the first on-screen window whose title
    /// contains `$SAKSHI_LIVE_WINDOW` (default "Sakshi": run the app with
    /// `pnpm tauri dev` first).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "records a real window; needs permissions, the sidecar, and models"]
    fn live_recording_through_the_app_backend() {
        use std::time::{Duration, Instant};

        let models = std::env::var_os("SAKSHI_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/dev-data")
            })
            .join("models");
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&models, root.path().join("models")).unwrap();
        let data = DataDir::new(root.path());
        let backend = Backend::new(
            data.clone(),
            Arc::new(sakshi_capture::platform),
            crate::processor(data.clone()),
            Arc::new(Recorded::default()),
        );

        let want = std::env::var("SAKSHI_LIVE_WINDOW").unwrap_or_else(|_| "Sakshi".into());
        let source = backend
            .sources()
            .unwrap()
            .into_iter()
            .find(|s| s.on_screen && s.title.contains(&want))
            .unwrap_or_else(|| panic!("no on-screen window titled {want:?}"));
        let status = backend
            .start_recording(StartRequest {
                source: source.id,
                title: Some("Live backend test".into()),
                microphone: false,
                system_audio: true,
            })
            .unwrap();
        let RecorderStatus::Recording { meeting_id, .. } = status else {
            panic!("not recording: {status:?}");
        };
        std::thread::sleep(Duration::from_secs(5));
        backend.recorder.stop().unwrap();

        let start = Instant::now();
        let job = loop {
            let job = backend.jobs.for_meeting(&meeting_id);
            if let Some(job) =
                job.filter(|j| matches!(j.state, JobState::Done | JobState::Failed { .. }))
            {
                break job;
            }
            assert!(
                start.elapsed() < Duration::from_secs(600),
                "processing took too long"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        let steps: Vec<String> = job
            .steps
            .iter()
            .map(|s| format!("{}={:?}", s.name, s.state))
            .collect();
        eprintln!("job: {:?}\nsteps: {}", job.state, steps.join(" "));
        assert_eq!(job.state, JobState::Done);
        assert_eq!(
            backend.recorder.status(),
            RecorderStatus::Idle { notice: None }
        );

        let detail = backend.meeting(&meeting_id).unwrap();
        eprintln!(
            "meeting: {} frames, {} segments, {:?} ms, recording {:?}",
            detail.frames.len(),
            detail.segments.len(),
            detail.item.duration_ms,
            detail.recording
        );
        assert_eq!(detail.item.status, MeetingStatus::Processed);
        assert!(detail.recording.is_some_and(|p| p.is_file()));
        assert!(!detail.frames.is_empty());
        let ms = detail.item.duration_ms.unwrap_or(0);
        assert!((4_000..8_000).contains(&ms), "duration {ms} ms");
    }
}
