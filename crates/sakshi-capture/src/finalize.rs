//! Turning a capture folder into the meeting's `recording.mp4`, `mic.wav`,
//! and `system.wav`.
//!
//! While recording, everything goes into `<meeting>/capture/` with a
//! `capture.json` manifest written up front. Finalizing works the same after
//! a clean stop or a crash, because every capture file is readable as it is
//! written.

use std::path::{Path, PathBuf};
use std::process::Command;

use sakshi_core::{MeetingDir, read_json};
use sakshi_media::Ffmpeg;
use serde::{Deserialize, Serialize};

use crate::{CaptureError, CaptureStats, Source};

/// Describes a capture in progress or finished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureManifest {
    /// RFC 3339 wall-clock time of the session start.
    pub started_at: String,
    pub source: Source,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Files relative to the meeting folder.
    pub video: String,
    pub system_audio: Option<String>,
    pub mic: Option<String>,
    /// Set by a clean stop; absent after a crash.
    pub stopped_at: Option<String>,
    pub stats: Option<CaptureStats>,
}

pub fn capture_dir(meeting: &MeetingDir) -> PathBuf {
    meeting.path().join("capture")
}

pub fn manifest_path(meeting: &MeetingDir) -> PathBuf {
    capture_dir(meeting).join("capture.json")
}

/// True when a capture exists that hasn't been turned into a recording yet,
/// e.g. after a crash.
pub fn needs_finalize(meeting: &MeetingDir) -> bool {
    manifest_path(meeting).exists() && !meeting.recording_mp4().exists()
}

fn non_empty(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
}

fn run(mut cmd: Command, what: &str, input: &Path) -> Result<(), CaptureError> {
    let out = cmd.output().map_err(|e| CaptureError::io(input, e))?;
    if !out.status.success() {
        return Err(sakshi_media::MediaError::FfmpegFailed {
            input: input.to_path_buf(),
            status: format!("{what}: {}", out.status),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }
        .into());
    }
    Ok(())
}

/// Produces `system.wav` and `mic.wav` (16 kHz mono PCM16) and
/// `recording.mp4` (the video with both tracks mixed, AAC). Removes the
/// large capture files afterwards unless `keep_raw` is set.
pub fn finalize(meeting: &MeetingDir, ffmpeg: &Ffmpeg, keep_raw: bool) -> Result<(), CaptureError> {
    let manifest: CaptureManifest = read_json(&manifest_path(meeting))?;
    let root = meeting.path();
    let raw = |file: &Option<String>| file.as_ref().map(|f| root.join(f)).filter(|p| non_empty(p));
    let system = raw(&manifest.system_audio);
    let mic = raw(&manifest.mic);
    let video = Some(root.join(&manifest.video)).filter(|p| non_empty(p));
    if system.is_none() && mic.is_none() && video.is_none() {
        return Err(CaptureError::NothingToFinalize {
            dir: capture_dir(meeting),
            reason: "the capture folder holds no audio or video".into(),
        });
    }

    for (input, output) in [(&system, meeting.system_wav()), (&mic, meeting.mic_wav())] {
        let Some(input) = input else { continue };
        let tmp = output.with_extension("partial.wav");
        let mut cmd = ffmpeg.command();
        cmd.arg("-y")
            .arg("-i")
            .arg(input)
            .args(["-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
            .arg(&tmp);
        run(cmd, "converting audio", input)?;
        std::fs::rename(&tmp, &output).map_err(|e| CaptureError::io(&output, e))?;
    }

    if let Some(video) = &video {
        let tmp = root.join("recording.partial.mp4");
        let mut cmd = ffmpeg.command();
        cmd.arg("-y").arg("-i").arg(video);
        let audio: Vec<&PathBuf> = [&system, &mic].into_iter().flatten().collect();
        for a in &audio {
            cmd.arg("-i").arg(a);
        }
        cmd.args(["-map", "0:v", "-c:v", "copy"]);
        match audio.len() {
            0 => {}
            1 => {
                cmd.args(["-map", "1:a"]);
            }
            _ => {
                cmd.args([
                    "-filter_complex",
                    "[1:a][2:a]amix=inputs=2:normalize=0:duration=longest[a]",
                    "-map",
                    "[a]",
                ]);
            }
        }
        if !audio.is_empty() {
            cmd.args(["-c:a", "aac", "-b:a", "128k"]);
        }
        cmd.args(["-movflags", "+faststart"]).arg(&tmp);
        run(cmd, "muxing recording.mp4", video)?;
        let out = meeting.recording_mp4();
        std::fs::rename(&tmp, &out).map_err(|e| CaptureError::io(&out, e))?;
    }

    if !keep_raw {
        for path in [system, mic, video].into_iter().flatten() {
            std::fs::remove_file(&path).map_err(|e| CaptureError::io(&path, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sakshi_core::write_json_atomic;

    use super::*;
    use crate::SourceId;

    fn source() -> Source {
        Source {
            id: SourceId::Window(7),
            title: "Zoom Meeting".into(),
            app_name: Some("zoom.us".into()),
            bundle_id: Some("us.zoom.xos".into()),
            width: 1280,
            height: 720,
            meeting: Some("Zoom".into()),
        }
    }

    fn sine(ffmpeg: &Ffmpeg, path: &Path, freq: u32, seconds: u32) {
        let status = ffmpeg
            .command()
            .args(["-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "sine=frequency={freq}:sample_rate=48000:duration={seconds}"
            ))
            .args(["-ac", "1", "-c:a", "pcm_s16le"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// ffmpeg's stream listing (printed at the default log level).
    fn streams(ffmpeg: &Ffmpeg, path: &Path) -> String {
        let out = Command::new(ffmpeg.path())
            .args(["-hide_banner", "-i"])
            .arg(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    #[test]
    fn builds_the_recording_and_processing_audio() {
        let ffmpeg = Ffmpeg::locate().expect("tests need ffmpeg");
        let dir = tempfile::tempdir().unwrap();
        let meeting = MeetingDir::new(dir.path());
        let capture = capture_dir(&meeting);
        std::fs::create_dir_all(&capture).unwrap();
        // A fragmented MP4 like the live encoder writes.
        let status = ffmpeg
            .command()
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x180:rate=15:duration=2",
            ])
            .args([
                "-c:v",
                "mpeg4",
                "-movflags",
                "+frag_keyframe+empty_moov+default_base_moof",
            ])
            .arg(capture.join("video.mp4"))
            .status()
            .unwrap();
        assert!(status.success());
        sine(&ffmpeg, &capture.join("system.wav"), 440, 2);
        sine(&ffmpeg, &capture.join("mic.wav"), 660, 2);
        write_json_atomic(
            &manifest_path(&meeting),
            &CaptureManifest {
                started_at: "2026-10-26T10:00:00Z".into(),
                source: source(),
                width: 320,
                height: 180,
                fps: 15,
                video: "capture/video.mp4".into(),
                system_audio: Some("capture/system.wav".into()),
                mic: Some("capture/mic.wav".into()),
                stopped_at: None,
                stats: None,
            },
        )
        .unwrap();
        assert!(needs_finalize(&meeting));

        finalize(&meeting, &ffmpeg, false).unwrap();

        for wav in [meeting.system_wav(), meeting.mic_wav()] {
            let spec = hound::WavReader::open(&wav).unwrap().spec();
            assert_eq!(
                (spec.sample_rate, spec.channels, spec.bits_per_sample),
                (16_000, 1, 16)
            );
        }
        let info = streams(&ffmpeg, &meeting.recording_mp4());
        assert!(
            info.contains("Video:") && info.contains("Audio: aac"),
            "{info}"
        );
        assert!(!needs_finalize(&meeting));
        assert!(!capture.join("video.mp4").exists(), "raw files removed");
        assert!(manifest_path(&meeting).exists(), "manifest kept");
    }

    #[test]
    fn a_capture_without_media_cannot_be_finalized() {
        let ffmpeg = Ffmpeg::locate().expect("tests need ffmpeg");
        let dir = tempfile::tempdir().unwrap();
        let meeting = MeetingDir::new(dir.path());
        write_json_atomic(
            &manifest_path(&meeting),
            &CaptureManifest {
                started_at: "2026-10-26T10:00:00Z".into(),
                source: source(),
                width: 2,
                height: 2,
                fps: 15,
                video: "capture/video.mp4".into(),
                system_audio: None,
                mic: None,
                stopped_at: None,
                stats: None,
            },
        )
        .unwrap();
        assert!(matches!(
            finalize(&meeting, &ffmpeg, false),
            Err(CaptureError::NothingToFinalize { .. })
        ));
    }
}
