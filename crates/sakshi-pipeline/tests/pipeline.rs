//! Runs the whole pipeline on a meeting folder with a fake transcriber.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use sakshi_asr::{AsrError, RawTranscript, RawWord, TranscribeRequest, Transcriber};
use sakshi_core::{Language, MeetingDir, Speaker, Track, Transcript, read_json};
use sakshi_media::Ffmpeg;
use sakshi_pipeline::{
    BleedConfig, Pipeline, PipelineConfig, PipelineError, PipelineState, RunOptions, Stage,
    StageEvent,
};

/// Returns canned words per track and records every call.
struct FakeTranscriber {
    calls: Mutex<Vec<(String, bool, String)>>,
    fail_mic: Mutex<bool>,
}

impl FakeTranscriber {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail_mic: Mutex::new(false),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.0.clone())
            .collect()
    }
}

fn word(start_ms: u64, text: &str, speaker: Option<u8>) -> RawWord {
    RawWord {
        start_ms,
        end_ms: start_ms + 250,
        text: text.into(),
        confidence: 1.0,
        speaker,
    }
}

fn words(start_ms: u64, text: &str, speaker: Option<u8>) -> Vec<RawWord> {
    text.split_whitespace()
        .enumerate()
        .map(|(i, t)| word(start_ms + i as u64 * 300, t, speaker))
        .collect()
}

impl Transcriber for FakeTranscriber {
    fn engine_id(&self) -> String {
        "fake 1".into()
    }

    fn transcribe(&self, req: &TranscribeRequest<'_>) -> Result<RawTranscript, AsrError> {
        let name = req
            .audio
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        self.calls
            .lock()
            .unwrap()
            .push((name.clone(), req.diarize, req.language.to_string()));
        let words = if name.starts_with("system") {
            [
                words(1000, "so what does everyone think", Some(2)),
                words(4000, "I guess we just see what Steve says", Some(5)),
                words(9000, "sounds right", Some(2)),
            ]
            .concat()
        } else {
            if std::mem::take(&mut *self.fail_mic.lock().unwrap()) {
                return Err(AsrError::NoDiarizationModel);
            }
            [
                // Bleed of the second system sentence.
                words(4100, "I guess we just see what Steve says", None),
                words(12_000, "I agree with that", None),
            ]
            .concat()
        };
        Ok(RawTranscript {
            languages: vec![],
            duration_ms: 15_000,
            words,
        })
    }
}

fn write_wav(path: &Path, sample_rate: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..sample_rate {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
}

fn meeting(dir: &Path) -> MeetingDir {
    write_wav(&dir.join("system.wav"), 16_000);
    write_wav(&dir.join("mic.wav"), 16_000);
    MeetingDir::new(dir)
}

fn transcript(meeting: &MeetingDir) -> Transcript {
    read_json(&meeting.transcript_json()).unwrap()
}

#[test]
fn produces_a_labelled_transcript_without_bleed() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();

    let report = Pipeline::new(&fake, None, PipelineConfig::default())
        .run(&meeting, &RunOptions::default())
        .unwrap();

    assert_eq!(report.ran, Stage::ALL);
    assert_eq!(report.bleed_removed, 1);
    assert_eq!(report.frames, 0);

    let t = transcript(&meeting);
    let lines: Vec<String> = t
        .segments
        .iter()
        .map(|s| format!("{} {} {} {}", s.id, s.start_ms, s.speaker, s.text))
        .collect();
    assert_eq!(
        lines,
        [
            "0 1000 Speaker 1 so what does everyone think",
            "1 4000 Speaker 2 I guess we just see what Steve says",
            "2 9000 Speaker 1 sounds right",
            "3 12000 You I agree with that",
        ]
    );
    assert_eq!(t.segments[3].track, Track::Mic);
    assert_eq!(t.segments[3].speaker, Speaker::You);

    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            ("system.wav".into(), true, "auto".into()),
            ("mic.wav".into(), false, "auto".into()),
        ]
    );
}

#[test]
fn a_second_run_reuses_finished_stages() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();
    let pipeline = Pipeline::new(&fake, None, PipelineConfig::default());

    pipeline.run(&meeting, &RunOptions::default()).unwrap();
    let report = pipeline.run(&meeting, &RunOptions::default()).unwrap();

    assert!(report.ran.is_empty());
    assert_eq!(report.skipped, Stage::ALL);
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn reports_progress_to_an_observer() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();
    *fake.fail_mic.lock().unwrap() = true;
    let events = Mutex::new(Vec::new());
    let observer = |e: &StageEvent| events.lock().unwrap().push(e.clone());

    let pipeline = Pipeline::new(&fake, None, PipelineConfig::default()).with_observer(&observer);
    pipeline.run(&meeting, &RunOptions::default()).unwrap_err();
    pipeline.run(&meeting, &RunOptions::default()).unwrap();

    let events = events.into_inner().unwrap();
    let kinds: Vec<String> = events
        .iter()
        .map(|e| match e {
            StageEvent::Started { stage } => format!("start {stage}"),
            StageEvent::Reused { stage } => format!("reuse {stage}"),
            StageEvent::Finished { stage, .. } => format!("done {stage}"),
            StageEvent::Failed { stage, .. } => format!("fail {stage}"),
        })
        .collect();
    assert_eq!(
        kinds[..7],
        [
            "start prepare-audio",
            "done prepare-audio",
            "start transcribe-system",
            "done transcribe-system",
            "start transcribe-mic",
            "fail transcribe-mic",
            "reuse prepare-audio",
        ]
    );
    assert_eq!(kinds.last().unwrap(), "done save");
    let json = serde_json::to_value(&events[1]).unwrap();
    assert_eq!(json["kind"], "finished");
    assert_eq!(json["stage"], "prepare-audio");
}

#[test]
fn resumes_after_a_failed_stage() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();
    *fake.fail_mic.lock().unwrap() = true;
    let pipeline = Pipeline::new(&fake, None, PipelineConfig::default());

    let err = pipeline.run(&meeting, &RunOptions::default()).unwrap_err();
    assert!(matches!(err, PipelineError::Asr(_)), "{err}");
    let state = PipelineState::load(&meeting.pipeline_json()).unwrap();
    assert!(state.get(Stage::TranscribeSystem).is_some());
    assert!(state.get(Stage::TranscribeMic).is_none());

    let report = pipeline.run(&meeting, &RunOptions::default()).unwrap();

    assert_eq!(
        report.skipped,
        [Stage::PrepareAudio, Stage::TranscribeSystem]
    );
    // The system track was transcribed once; only the mic was retried.
    assert_eq!(fake.calls(), ["system.wav", "mic.wav", "mic.wav"]);
    assert_eq!(report.segments, 4);
}

#[test]
fn changed_settings_redo_only_the_affected_stages() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();
    Pipeline::new(&fake, None, PipelineConfig::default())
        .run(&meeting, &RunOptions::default())
        .unwrap();

    // Thresholds nothing can reach keep the bleed.
    let config = PipelineConfig {
        bleed: BleedConfig {
            threshold: 1.1,
            min_run_words: usize::MAX,
            ..BleedConfig::default()
        },
        ..PipelineConfig::default()
    };
    let report = Pipeline::new(&fake, None, config)
        .run(&meeting, &RunOptions::default())
        .unwrap();

    assert_eq!(report.ran, [Stage::RemoveBleed, Stage::Save]);
    assert_eq!(report.bleed_removed, 0);
    assert_eq!(report.segments, 5);
    assert_eq!(fake.calls().len(), 2);

    // Pinning a language redoes transcription and everything after it.
    let config = PipelineConfig {
        language: Language::Code("en".into()),
        ..PipelineConfig::default()
    };
    let report = Pipeline::new(&fake, None, config)
        .run(&meeting, &RunOptions::default())
        .unwrap();
    assert_eq!(
        report.ran,
        [
            Stage::TranscribeSystem,
            Stage::TranscribeMic,
            Stage::Segments,
            Stage::RemoveBleed,
            Stage::Save
        ]
    );
    assert_eq!(report.languages, ["en"]);
}

#[test]
fn from_reruns_a_stage_and_its_dependents() {
    let dir = tempfile::tempdir().unwrap();
    let meeting = meeting(dir.path());
    let fake = FakeTranscriber::new();
    let pipeline = Pipeline::new(&fake, None, PipelineConfig::default());
    pipeline.run(&meeting, &RunOptions::default()).unwrap();

    let report = pipeline
        .run(
            &meeting,
            &RunOptions {
                from: Some(Stage::Segments),
                force: false,
            },
        )
        .unwrap();

    assert_eq!(
        report.ran,
        [Stage::Segments, Stage::RemoveBleed, Stage::Save]
    );
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn a_missing_track_is_recorded_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(&dir.path().join("system.wav"), 16_000);
    let meeting = MeetingDir::new(dir.path());
    let fake = FakeTranscriber::new();

    let report = Pipeline::new(&fake, None, PipelineConfig::default())
        .run(&meeting, &RunOptions::default())
        .unwrap();

    assert_eq!(fake.calls(), ["system.wav"]);
    assert_eq!(report.segments, 3);
    let state = PipelineState::load(&meeting.pipeline_json()).unwrap();
    assert_eq!(
        state.get(Stage::TranscribeMic).unwrap().note.as_deref(),
        Some("no mic.wav")
    );
}

#[test]
fn no_audio_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeTranscriber::new();
    let err = Pipeline::new(&fake, None, PipelineConfig::default())
        .run(&MeetingDir::new(dir.path()), &RunOptions::default())
        .unwrap_err();
    assert!(matches!(err, PipelineError::NoAudio { .. }), "{err}");
}

#[test]
fn audio_that_needs_conversion_requires_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(&dir.path().join("system.wav"), 48_000);
    let fake = FakeTranscriber::new();

    let err = Pipeline::new(&fake, None, PipelineConfig::default())
        .run(&MeetingDir::new(dir.path()), &RunOptions::default())
        .unwrap_err();

    assert!(err.to_string().contains("ffmpeg not found"), "{err}");
}

#[test]
fn converts_audio_and_extracts_frames_with_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    write_wav(&dir.path().join("system.wav"), 44_100);
    let ffmpeg = Ffmpeg::locate().expect("tests need ffmpeg");
    let status = Command::new(ffmpeg.path())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=5:duration=3",
        ])
        .args(["-c:v", "mpeg4"])
        .arg(dir.path().join("recording.mp4"))
        .status()
        .unwrap();
    assert!(status.success());
    let meeting = MeetingDir::new(dir.path());
    let fake = FakeTranscriber::new();

    let report = Pipeline::new(&fake, Some(ffmpeg), PipelineConfig::default())
        .run(&meeting, &RunOptions::default())
        .unwrap();

    // The 44.1 kHz track was converted and the converted copy transcribed.
    assert_eq!(fake.calls(), ["system.16k.wav"]);
    assert!(meeting.work_dir().join("system.16k.wav").is_file());
    assert!(report.frames >= 1);
    assert!(meeting.frames_dir().join("0.jpg").is_file());
}
