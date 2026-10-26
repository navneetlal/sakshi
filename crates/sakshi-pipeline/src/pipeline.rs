use std::path::{Path, PathBuf};

use sakshi_asr::{RawTranscript, TranscribeRequest, Transcriber};
use sakshi_core::{
    Frame, Language, MeetingDir, Segment, Track, Transcript, read_json, write_json_atomic,
};
use sakshi_media::{Ffmpeg, KeyframeConfig, MediaError, wav_info};

use crate::PipelineError;
use crate::bleed::{BleedConfig, BleedMatch, remove_bleed};
use crate::segments::build_segments;
use crate::state::{PipelineState, Stage};

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub language: Language,
    /// A pause longer than this starts a new segment.
    pub segment_gap_ms: u64,
    pub bleed: BleedConfig,
    pub keyframes: KeyframeConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            segment_gap_ms: 1000,
            bleed: BleedConfig::default(),
            keyframes: KeyframeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Run this stage again, and every stage that reads its output.
    pub from: Option<Stage>,
    /// Run every stage again.
    pub force: bool,
}

/// What a run did and produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub ran: Vec<Stage>,
    pub skipped: Vec<Stage>,
    pub segments: usize,
    pub bleed_removed: usize,
    pub frames: usize,
    pub languages: Vec<String>,
}

/// Turns a meeting folder's recordings into `transcript.json` and key
/// frames. Every stage records its completion in `pipeline.json`, so an
/// interrupted run resumes after the last finished stage.
pub struct Pipeline<'a> {
    transcriber: &'a dyn Transcriber,
    ffmpeg: Option<Ffmpeg>,
    config: PipelineConfig,
}

impl<'a> Pipeline<'a> {
    /// `ffmpeg` is only needed when audio must be converted or a video is
    /// present; without it those stages fail with an install hint.
    pub fn new(
        transcriber: &'a dyn Transcriber,
        ffmpeg: Option<Ffmpeg>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            transcriber,
            ffmpeg,
            config,
        }
    }

    pub fn run(&self, meeting: &MeetingDir, options: &RunOptions) -> Result<Report, PipelineError> {
        if !meeting.mic_wav().exists() && !meeting.system_wav().exists() {
            return Err(PipelineError::NoAudio {
                dir: meeting.path().to_path_buf(),
            });
        }
        let state_path = meeting.pipeline_json();
        let mut state = PipelineState::load(&state_path)?;
        let mut ran: Vec<Stage> = Vec::new();
        let mut skipped = Vec::new();

        for stage in Stage::ALL {
            let key = self.key(stage);
            let forced = options.force || options.from == Some(stage);
            let input_ran = stage.inputs().iter().any(|i| ran.contains(i));
            let fresh = state.get(stage).is_some_and(|r| r.key == key);
            if fresh && !forced && !input_ran {
                tracing::info!(%stage, "already done");
                skipped.push(stage);
                continue;
            }
            tracing::info!(%stage, "running");
            let note = self.run_stage(stage, meeting)?;
            if let Some(note) = &note {
                tracing::info!(%stage, "{note}");
            }
            state.record(stage, key, note);
            state.save(&state_path)?;
            ran.push(stage);
        }

        let transcript: Transcript = read_json(&meeting.transcript_json())?;
        let frames: Vec<Frame> = read_json(&meeting.frames_json())?;
        let removed: Vec<BleedMatch> = read_json(&work(meeting, "bleed-removed.json"))?;
        Ok(Report {
            ran,
            skipped,
            segments: transcript.segments.len(),
            bleed_removed: removed.len(),
            frames: frames.len(),
            languages: transcript.languages,
        })
    }

    /// The settings a stage's output depends on.
    fn key(&self, stage: Stage) -> String {
        let c = &self.config;
        match stage {
            Stage::PrepareAudio | Stage::Save => "v1".into(),
            Stage::TranscribeSystem | Stage::TranscribeMic => {
                format!("{} language={}", self.transcriber.engine_id(), c.language)
            }
            Stage::Segments => format!("gap_ms={}", c.segment_gap_ms),
            Stage::RemoveBleed => format!(
                "threshold={} window_ms={} min_words={}",
                c.bleed.threshold, c.bleed.window_ms, c.bleed.min_words
            ),
            Stage::Keyframes => format!("{:?}", c.keyframes),
        }
    }

    fn run_stage(
        &self,
        stage: Stage,
        meeting: &MeetingDir,
    ) -> Result<Option<String>, PipelineError> {
        match stage {
            Stage::PrepareAudio => self.prepare_audio(meeting),
            Stage::TranscribeSystem => self.transcribe(meeting, Track::System),
            Stage::TranscribeMic => self.transcribe(meeting, Track::Mic),
            Stage::Segments => self.segments(meeting),
            Stage::RemoveBleed => self.remove_bleed(meeting),
            Stage::Keyframes => self.keyframes(meeting),
            Stage::Save => self.save(meeting),
        }
    }

    fn ffmpeg(&self) -> Result<&Ffmpeg, PipelineError> {
        self.ffmpeg.as_ref().ok_or_else(|| {
            MediaError::FfmpegNotFound {
                env: sakshi_media::FFMPEG_ENV,
            }
            .into()
        })
    }

    /// Converts tracks that aren't already 16 kHz mono into `work/`.
    fn prepare_audio(&self, meeting: &MeetingDir) -> Result<Option<String>, PipelineError> {
        let mut notes = Vec::new();
        for track in [Track::System, Track::Mic] {
            let original = original_audio(meeting, track);
            if !original.exists() {
                notes.push(format!("no {track}.wav"));
                continue;
            }
            let converted = converted_audio(meeting, track);
            match wav_info(&original) {
                Ok(info) if info.is_pipeline_ready() => {
                    remove_if_exists(&converted)?;
                    notes.push(format!("{track}: ready"));
                }
                other => {
                    let from = other.map_or_else(
                        |_| "unrecognized WAV".to_owned(),
                        |i| format!("{} Hz, {} ch", i.sample_rate, i.channels),
                    );
                    self.ffmpeg()?.resample_to_wav(&original, &converted)?;
                    notes.push(format!("{track}: converted from {from}"));
                }
            }
        }
        Ok(Some(notes.join("; ")))
    }

    fn transcribe(
        &self,
        meeting: &MeetingDir,
        track: Track,
    ) -> Result<Option<String>, PipelineError> {
        let words_path = work(meeting, &format!("{track}.words.json"));
        let Some(audio) = track_audio(meeting, track) else {
            write_json_atomic(&words_path, &RawTranscript::default())?;
            return Ok(Some(format!("no {track}.wav")));
        };
        let raw_output = work(meeting, &format!("{track}.engine.json"));
        let raw = self.transcriber.transcribe(&TranscribeRequest {
            audio: &audio,
            language: &self.config.language,
            diarize: track == Track::System,
            raw_output: &raw_output,
        })?;
        write_json_atomic(&words_path, &raw)?;
        Ok(Some(format!("{} words", raw.words.len())))
    }

    fn segments(&self, meeting: &MeetingDir) -> Result<Option<String>, PipelineError> {
        let mut segments = Vec::new();
        for track in [Track::System, Track::Mic] {
            let raw: RawTranscript = read_json(&work(meeting, &format!("{track}.words.json")))?;
            segments.extend(build_segments(
                &raw.words,
                track,
                self.config.segment_gap_ms,
            ));
        }
        segments.sort_by_key(|s| (s.start_ms, s.end_ms, s.track == Track::System));
        number(&mut segments);
        write_json_atomic(&work(meeting, "segments.json"), &segments)?;
        Ok(Some(format!("{} segments", segments.len())))
    }

    fn remove_bleed(&self, meeting: &MeetingDir) -> Result<Option<String>, PipelineError> {
        let segments: Vec<Segment> = read_json(&work(meeting, "segments.json"))?;
        let (mut kept, removed) = remove_bleed(segments, &self.config.bleed);
        number(&mut kept);
        write_json_atomic(&work(meeting, "segments.clean.json"), &kept)?;
        write_json_atomic(&work(meeting, "bleed-removed.json"), &removed)?;
        Ok(Some(format!(
            "removed {} mic segments as bleed",
            removed.len()
        )))
    }

    fn keyframes(&self, meeting: &MeetingDir) -> Result<Option<String>, PipelineError> {
        let video = meeting.recording_mp4();
        let frames_dir = meeting.frames_dir();
        if frames_dir.exists() {
            std::fs::remove_dir_all(&frames_dir).map_err(|e| PipelineError::io(&frames_dir, e))?;
        }
        if !video.exists() {
            write_json_atomic(&meeting.frames_json(), &Vec::<Frame>::new())?;
            return Ok(Some("no recording.mp4".into()));
        }
        let frames =
            self.ffmpeg()?
                .extract_keyframes(&video, &frames_dir, &self.config.keyframes)?;
        write_json_atomic(&meeting.frames_json(), &frames)?;
        Ok(Some(format!("{} key frames", frames.len())))
    }

    fn save(&self, meeting: &MeetingDir) -> Result<Option<String>, PipelineError> {
        let segments: Vec<Segment> = read_json(&work(meeting, "segments.clean.json"))?;
        let languages = match &self.config.language {
            Language::Code(code) => vec![code.clone()],
            Language::Auto => {
                let mut reported = Vec::new();
                for track in [Track::System, Track::Mic] {
                    let raw: RawTranscript =
                        read_json(&work(meeting, &format!("{track}.words.json")))?;
                    for language in raw.languages {
                        if !reported.contains(&language) {
                            reported.push(language);
                        }
                    }
                }
                reported
            }
        };
        let transcript = Transcript {
            schema_version: Transcript::SCHEMA_VERSION,
            languages,
            segments,
        };
        write_json_atomic(&meeting.transcript_json(), &transcript)?;
        Ok(Some(format!("{} segments", transcript.segments.len())))
    }
}

fn work(meeting: &MeetingDir, name: &str) -> PathBuf {
    meeting.work_dir().join(name)
}

fn original_audio(meeting: &MeetingDir, track: Track) -> PathBuf {
    match track {
        Track::Mic => meeting.mic_wav(),
        Track::System => meeting.system_wav(),
    }
}

fn converted_audio(meeting: &MeetingDir, track: Track) -> PathBuf {
    work(meeting, &format!("{track}.16k.wav"))
}

/// The audio to transcribe for `track`: the converted copy if one was made,
/// else the original, else `None` when the track wasn't recorded.
fn track_audio(meeting: &MeetingDir, track: Track) -> Option<PathBuf> {
    let converted = converted_audio(meeting, track);
    let original = original_audio(meeting, track);
    [converted, original].into_iter().find(|p| p.exists())
}

fn remove_if_exists(path: &Path) -> Result<(), PipelineError> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(PipelineError::io(path, e)),
        _ => Ok(()),
    }
}

fn number(segments: &mut [Segment]) {
    for (id, segment) in (0u32..).zip(segments.iter_mut()) {
        segment.id = id;
    }
}
