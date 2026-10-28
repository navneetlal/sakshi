//! Recordings made elsewhere (Zoom, Fireflies, ...) become meetings: the
//! file is copied into `meetings/<id>/` as the pipeline expects it, then
//! processed like a recording.

use std::path::Path;
use std::process::Command;

use sakshi_core::{
    DataDir, ImportRequest, ImportState, Language, MeetingDir, MeetingInfo, read_json,
    write_json_atomic,
};
use sakshi_media::{Ffmpeg, MediaError};

use crate::AppError;

/// Queues `source` for import and returns the request; the meeting will
/// have the request's id. The app picks requests up, and
/// [`process_meeting`](crate::process_meeting) on the meeting's folder runs
/// one directly.
pub fn request_import(
    data: &DataDir,
    source: &Path,
    title: Option<String>,
    started_at: Option<String>,
    language: Option<Language>,
) -> Result<ImportRequest, AppError> {
    let source = std::path::absolute(source).map_err(|e| AppError::io(source, e))?;
    let source = ImportRequest::check_source(&source)?;
    if let Some(t) = &started_at {
        t.parse::<jiff::Timestamp>()
            .map_err(|e| AppError::Invalid(format!("invalid start time {t:?}: {e}")))?;
    }
    let request = ImportRequest {
        id: uuid::Uuid::now_v7().to_string(),
        source,
        title: title.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()),
        started_at,
        language,
        requested_at: jiff::Timestamp::now().to_string(),
        state: ImportState::Queued,
    };
    request.save(data)?;
    Ok(request)
}

/// The meeting folder for an import, with its `meeting.json`, so the
/// meeting is listed while it waits. Leaves an existing one alone.
pub fn import_folder(data: &DataDir, request: &ImportRequest) -> Result<MeetingDir, AppError> {
    let meeting = data.meeting(&request.id);
    if meeting.info_json().is_file() {
        return Ok(meeting);
    }
    let file = request.file_name();
    let title = request.title.clone().unwrap_or_else(|| {
        Path::new(&file)
            .file_stem()
            .map_or_else(|| file.clone(), |s| s.to_string_lossy().into_owned())
    });
    let info = MeetingInfo {
        id: Some(request.id.clone()),
        title: Some(title),
        started_at: request.started_at.clone(),
        language: request.language.clone(),
        imported_from: Some(file),
        ..MeetingInfo::default()
    };
    write_json_atomic(&meeting.info_json(), &info)?;
    Ok(meeting)
}

/// Copies the recording into its meeting folder: any video, with its audio,
/// as `recording.mp4`, then the audio as `system.wav`, which is written last
/// so its presence means the import finished. Takes the start time from
/// the file when the request had none.
pub(crate) fn import_media(
    data: &DataDir,
    request: &ImportRequest,
    ffmpeg: &Ffmpeg,
) -> Result<MeetingDir, AppError> {
    let meeting = import_folder(data, request)?;
    let source = ImportRequest::check_source(&request.source)?;
    let media = ffmpeg.probe(&source)?;
    if !media.has_audio {
        return Err(AppError::Invalid(format!(
            "{} has no audio to transcribe",
            request.file_name()
        )));
    }
    if media.has_video {
        copy_video(ffmpeg, &source, &meeting.recording_mp4())?;
    }
    ffmpeg.resample_to_wav(&source, &meeting.system_wav())?;

    let mut info: MeetingInfo = read_json(&meeting.info_json())?;
    if info.started_at.is_none()
        && let Some(t) = media
            .creation_time
            .and_then(|t| t.parse::<jiff::Timestamp>().ok())
    {
        info.started_at = Some(t.to_string());
        write_json_atomic(&meeting.info_json(), &info)?;
    }
    Ok(meeting)
}

/// The first video stream and the first audio stream as an MP4 the app can
/// play. The video is copied when MP4 can hold it, else re-encoded.
fn copy_video(ffmpeg: &Ffmpeg, source: &Path, output: &Path) -> Result<(), AppError> {
    let tmp = output.with_extension("partial.mp4");
    let encoder: &[&str] = if cfg!(target_os = "macos") {
        &["-c:v", "h264_videotoolbox", "-b:v", "2500k"]
    } else {
        &["-c:v", "libx264", "-preset", "veryfast", "-crf", "26"]
    };
    let mut last = None;
    for (video, encode) in [(&["-c:v", "copy"][..], false), (encoder, true)] {
        let mut cmd = ffmpeg.command();
        cmd.arg("-y")
            .arg("-i")
            .arg(source)
            .args(["-map", "0:v:0", "-map", "0:a:0"])
            .args(video);
        if encode {
            cmd.args(["-pix_fmt", "yuv420p"]);
        }
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"])
            .arg(&tmp);
        match run(cmd, source) {
            Ok(()) => {
                return std::fs::rename(&tmp, output).map_err(|e| AppError::io(output, e));
            }
            Err(e) => last = Some(e),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.expect("tried at least once"))
}

fn run(mut cmd: Command, input: &Path) -> Result<(), AppError> {
    let out = cmd.output().map_err(|e| AppError::io(input, e))?;
    if !out.status.success() {
        return Err(MediaError::FfmpegFailed {
            input: input.to_path_buf(),
            status: format!("copying the video: {}", out.status),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sakshi_media::wav_info;

    use super::*;

    fn ffmpeg() -> Ffmpeg {
        Ffmpeg::locate().expect("tests need ffmpeg")
    }

    /// A short clip made by ffmpeg: video unless `audio_only`, with a tone.
    fn clip(dir: &Path, name: &str, audio_only: bool) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut cmd = ffmpeg().command();
        if !audio_only {
            cmd.args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=5:duration=2",
            ]);
        }
        cmd.args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=2",
        ])
        .args([
            "-metadata",
            "creation_time=2026-10-20T04:00:00Z",
            "-shortest",
        ])
        .arg(&path);
        assert!(cmd.status().unwrap().success());
        path
    }

    #[test]
    fn imports_a_video_and_keeps_only_its_file_name() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path().join("data"));
        let source = clip(root.path(), "Weekly sync.mp4", false);

        let request = request_import(&data, &source, None, None, None).unwrap();
        assert_eq!(
            ImportRequest::load(&data, &request.id).unwrap(),
            Some(request.clone())
        );
        let meeting = import_media(&data, &request, &ffmpeg()).unwrap();

        assert!(meeting.recording_mp4().is_file());
        let wav = wav_info(&meeting.system_wav()).unwrap();
        assert!(wav.is_pipeline_ready() && (1900..=2100).contains(&wav.duration_ms));
        assert!(!meeting.path().join("recording.partial.mp4").exists());
        let info: MeetingInfo = read_json(&meeting.info_json()).unwrap();
        assert_eq!(info.id.as_deref(), Some(request.id.as_str()));
        assert_eq!(info.title.as_deref(), Some("Weekly sync"));
        assert_eq!(info.imported_from.as_deref(), Some("Weekly sync.mp4"));
        assert_eq!(info.started_at.as_deref(), Some("2026-10-20T04:00:00Z"));
        assert!(source.is_file(), "the original stays");
    }

    #[test]
    fn imports_audio_only_and_respects_the_requested_details() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path().join("data"));
        let source = clip(root.path(), "call.m4a", true);

        let request = request_import(
            &data,
            &source,
            Some(" Design review ".into()),
            Some("2026-10-27T10:00:00+05:30".into()),
            Some(Language::Code("hi".into())),
        )
        .unwrap();
        let meeting = import_media(&data, &request, &ffmpeg()).unwrap();

        assert!(
            !meeting.recording_mp4().exists(),
            "no video, no recording.mp4"
        );
        assert!(meeting.system_wav().is_file());
        let info: MeetingInfo = read_json(&meeting.info_json()).unwrap();
        assert_eq!(info.title.as_deref(), Some("Design review"));
        assert_eq!(
            info.started_at.as_deref(),
            Some("2026-10-27T10:00:00+05:30")
        );
        assert_eq!(info.language, Some(Language::Code("hi".into())));
    }

    #[test]
    fn rejects_what_it_cannot_import() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path().join("data"));
        assert!(request_import(&data, &root.path().join("gone.mp4"), None, None, None).is_err());
        let notes = root.path().join("notes.txt");
        std::fs::write(&notes, "not media").unwrap();
        assert!(request_import(&data, &notes, None, Some("yesterday".into()), None).is_err());

        let request = request_import(&data, &notes, None, None, None).unwrap();
        let err = import_media(&data, &request, &ffmpeg()).unwrap_err();
        assert!(matches!(err, AppError::Media(_)), "{err}");
        assert!(!data.meeting(&request.id).system_wav().exists());
    }
}
