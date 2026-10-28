//! The Sakshi desktop app. The window is a React UI; everything it does
//! goes through the commands below, which wrap [`backend::Backend`].

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod events;
mod jobs;
mod models;
mod recorder;
#[cfg(test)]
mod test_util;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use backend::{
    Backend, McpView, MeetingDetail, MeetingItem, PermissionKind, PermissionsView, SettingsView,
    SetupCheck,
};
use jobs::{JobView, Processor};
use models::{InspectedModel, ModelView};
use recorder::{CaptureFactory, RecorderStatus, StartRequest};
use sakshi_app::mcp_setup::{Client, InstallOutcome};
use sakshi_capture::Source;
use sakshi_core::{DataDir, Language, SpeakerNames};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tracing_subscriber::EnvFilter;

type Shared<'a> = State<'a, Arc<Backend>>;

/// How often the app looks for imports the MCP server queued.
const IMPORT_POLL: Duration = Duration::from_secs(2);

/// What the import dialog offers; ffmpeg reads more, and anything it can't
/// is reported when the import runs.
const RECORDING_TYPES: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "webm", "avi", "m4a", "mp3", "wav", "aac", "ogg", "opus", "flac",
];

/// Runs `f` off the async runtime: most commands touch the disk, the
/// database, or the OS's capture APIs, which can block.
async fn blocking<T: Send + 'static>(
    backend: &Arc<Backend>,
    f: impl FnOnce(&Backend) -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    let backend = Arc::clone(backend);
    tauri::async_runtime::spawn_blocking(move || f(&backend))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

fn language(code: Option<String>) -> anyhow::Result<Option<Language>> {
    code.map(|c| c.parse::<Language>().map_err(anyhow::Error::msg))
        .transpose()
}

#[tauri::command]
async fn permissions(backend: Shared<'_>) -> Result<PermissionsView, String> {
    blocking(&backend, |b| Ok(b.permissions())).await
}

#[tauri::command]
async fn request_permission(
    backend: Shared<'_>,
    kind: PermissionKind,
) -> Result<PermissionsView, String> {
    blocking(&backend, move |b| Ok(b.request_permission(kind))).await
}

/// Opens the System Settings page where the user turns access on.
#[tauri::command]
fn open_privacy_settings(kind: PermissionKind) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("recording isn't available on this platform yet".into());
    }
    let pane = match kind {
        PermissionKind::Screen => "Privacy_ScreenCapture",
        PermissionKind::Microphone => "Privacy_Microphone",
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Opens a web page, such as a model's license, in the browser.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("not opening {url:?}: only https links open"));
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_data_dir(backend: Shared<'_>) -> Result<(), String> {
    tauri_plugin_opener::open_path(backend.data.root(), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_sources(backend: Shared<'_>) -> Result<Vec<Source>, String> {
    blocking(&backend, |b| b.sources()).await
}

#[tauri::command]
fn recorder_status(backend: Shared<'_>) -> RecorderStatus {
    backend.recorder.status()
}

#[tauri::command]
async fn start_recording(
    backend: Shared<'_>,
    request: StartRequest,
) -> Result<RecorderStatus, String> {
    blocking(&backend, move |b| b.start_recording(request)).await
}

#[tauri::command]
fn stop_recording(backend: Shared<'_>) -> Result<(), String> {
    backend.recorder.stop().map_err(|e| format!("{e:#}"))
}

/// Synchronous, so it runs on the main thread while Sakshi is still the
/// active app from the click; macOS only lets the active app hand focus on.
#[tauri::command]
fn show_recorded_window(backend: Shared<'_>) -> Result<(), String> {
    backend.recorder.show().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn list_meetings(
    backend: Shared<'_>,
    query: Option<String>,
) -> Result<Vec<MeetingItem>, String> {
    blocking(&backend, move |b| b.meetings(query.as_deref())).await
}

#[tauri::command]
async fn get_meeting(
    app: tauri::AppHandle,
    backend: Shared<'_>,
    id: String,
) -> Result<MeetingDetail, String> {
    let detail = blocking(&backend, move |b| b.meeting(&id)).await?;
    // Meetings processed from elsewhere with `sakshi-cli process` live
    // outside the data folder; let the UI show their frames and video too.
    app.asset_protocol_scope()
        .allow_directory(&detail.folder, true)
        .map_err(|e| e.to_string())?;
    Ok(detail)
}

#[tauri::command]
async fn process_meeting(
    backend: Shared<'_>,
    id: String,
    language: Option<String>,
    force: bool,
) -> Result<JobView, String> {
    blocking(&backend, move |b| {
        b.process(&id, self::language(language)?, force)
    })
    .await
}

#[tauri::command]
async fn set_meeting_language(
    backend: Shared<'_>,
    id: String,
    language: Option<String>,
) -> Result<(), String> {
    blocking(&backend, move |b| {
        b.set_meeting_language(&id, self::language(language)?)
    })
    .await
}

/// Names speakers by label; a blank name removes one.
#[tauri::command]
async fn set_speaker_names(
    backend: Shared<'_>,
    id: String,
    names: BTreeMap<String, String>,
) -> Result<SpeakerNames, String> {
    let names = names.into_iter().map(|(l, n)| (l, Some(n))).collect();
    blocking(&backend, move |b| b.set_speaker_names(&id, names)).await
}

/// Asks for a recording made elsewhere and imports it. Returns the new
/// meeting's id, or nothing if the user cancelled.
#[tauri::command]
async fn import_recording(
    app: tauri::AppHandle,
    backend: Shared<'_>,
) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Import a recording")
            .add_filter("Video or audio", RECORDING_TYPES)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(file) = picked else {
        return Ok(None);
    };
    let file = file.into_path().map_err(|e| e.to_string())?;
    blocking(&backend, move |b| b.import(&file)).await.map(Some)
}

#[tauri::command]
async fn reveal_meeting(backend: Shared<'_>, id: String) -> Result<(), String> {
    blocking(&backend, move |b| {
        let folder = b.meeting_folder(&id)?;
        tauri_plugin_opener::reveal_item_in_dir(&folder)
            .with_context(|| format!("showing {}", folder.display()))
    })
    .await
}

#[tauri::command]
fn list_jobs(backend: Shared<'_>) -> Vec<JobView> {
    backend.jobs.list()
}

#[tauri::command]
async fn list_models(backend: Shared<'_>) -> Result<Vec<ModelView>, String> {
    blocking(&backend, |b| b.models.list()).await
}

#[tauri::command]
async fn download_model(
    backend: Shared<'_>,
    id: String,
    license_accepted: bool,
) -> Result<(), String> {
    blocking(&backend, move |b| b.models.download(&id, license_accepted)).await
}

#[tauri::command]
async fn inspect_model(
    backend: Shared<'_>,
    repo: String,
    file: String,
    revision: Option<String>,
) -> Result<InspectedModel, String> {
    blocking(&backend, move |b| {
        b.models.inspect(
            &repo,
            &file,
            revision.as_deref().filter(|r| !r.trim().is_empty()),
        )
    })
    .await
}

#[tauri::command]
async fn add_model(backend: Shared<'_>, id: String, license_accepted: bool) -> Result<(), String> {
    blocking(&backend, move |b| b.models.add(&id, license_accepted)).await
}

#[tauri::command]
async fn get_settings(backend: Shared<'_>) -> Result<SettingsView, String> {
    blocking(&backend, |b| b.settings()).await
}

#[tauri::command]
async fn save_settings(
    backend: Shared<'_>,
    language: String,
    consent_reminder: bool,
) -> Result<SettingsView, String> {
    blocking(&backend, move |b| {
        let language = self::language(Some(language))?.unwrap_or_default();
        b.save_settings(language, consent_reminder)
    })
    .await
}

#[tauri::command]
async fn setup_check(backend: Shared<'_>) -> Result<SetupCheck, String> {
    blocking(&backend, |b| Ok(b.setup_check())).await
}

#[tauri::command]
async fn mcp_status(backend: Shared<'_>) -> Result<McpView, String> {
    blocking(&backend, |b| Ok(b.mcp())).await
}

#[tauri::command]
async fn mcp_install(backend: Shared<'_>, client: Client) -> Result<InstallOutcome, String> {
    blocking(&backend, move |b| b.mcp_install(client)).await
}

fn processor(data: DataDir) -> Arc<Processor> {
    Arc::new(move |folder, options, progress| {
        let outcome = sakshi_app::process_meeting(&data, folder, options, progress)?;
        Ok(outcome.meeting.id)
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data = DataDir::from_env_or_default()?;
            let meetings = data.meetings_dir();
            std::fs::create_dir_all(&meetings)
                .with_context(|| format!("creating {}", meetings.display()))?;
            // Key frames and recordings are shown straight from the files.
            app.asset_protocol_scope()
                .allow_directory(&meetings, true)?;
            let capture: Arc<CaptureFactory> = Arc::new(sakshi_capture::platform);
            let backend = Arc::new(Backend::new(
                data.clone(),
                capture,
                processor(data),
                Arc::new(app.handle().clone()),
            ));
            app.manage(Arc::clone(&backend));
            // Imports the MCP server queued, or that the last run left.
            std::thread::Builder::new()
                .name("sakshi-imports".into())
                .spawn(move || {
                    loop {
                        if let Err(e) = backend.take_imports() {
                            tracing::warn!("looking for imports: {e:#}");
                        }
                        std::thread::sleep(IMPORT_POLL);
                    }
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            permissions,
            request_permission,
            open_privacy_settings,
            open_link,
            reveal_data_dir,
            list_sources,
            recorder_status,
            start_recording,
            stop_recording,
            show_recorded_window,
            list_meetings,
            get_meeting,
            process_meeting,
            set_meeting_language,
            set_speaker_names,
            import_recording,
            reveal_meeting,
            list_jobs,
            list_models,
            download_model,
            inspect_model,
            add_model,
            get_settings,
            save_settings,
            setup_check,
            mcp_status,
            mcp_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sakshi");
}
