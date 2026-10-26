use std::path::Path;

use sakshi_core::Language;
use serde::{Deserialize, Serialize};

use crate::AsrError;

/// Turns one audio track into timed words.
pub trait Transcriber: Send + Sync {
    fn transcribe(&self, request: &TranscribeRequest<'_>) -> Result<RawTranscript, AsrError>;

    /// Names the engine, its version, and its models, so results can be
    /// traced back and redone when any of them change.
    fn engine_id(&self) -> String;
}

#[derive(Debug, Clone, Copy)]
pub struct TranscribeRequest<'a> {
    /// A WAV file; 16 kHz mono is what the pipeline provides.
    pub audio: &'a Path,
    pub language: &'a Language,
    /// Tag each word with a speaker.
    pub diarize: bool,
    /// Where the engine's own output is kept, for debugging.
    pub raw_output: &'a Path,
}

/// Recognizer output before segmentation and speaker labelling.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawTranscript {
    /// Languages the engine reported, if any.
    pub languages: Vec<String>,
    pub duration_ms: u64,
    pub words: Vec<RawWord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: f32,
    /// The engine's speaker index (from 1) when diarizing; `None` when no
    /// speaker was assigned.
    pub speaker: Option<u8>,
}
