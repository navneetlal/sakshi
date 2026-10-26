//! Speech recognition and speaker diarization through the pinned
//! NeMo-Speech.cpp `nemo-speech` sidecar.

mod error;
mod languages;
mod nemo;
mod output;
mod sidecar;
mod transcriber;

pub use error::AsrError;
pub use languages::LanguageTable;
pub use nemo::{NemoSpeech, RIGHT_CONTEXT};
pub use output::parse_transcribe_json;
pub use sidecar::{SIDECAR_ENV, locate_sidecar};
pub use transcriber::{RawTranscript, RawWord, TranscribeRequest, Transcriber};

/// The NeMo-Speech.cpp release Sakshi is built and tested against. The model
/// revisions in the `sakshi-models` manifest are pinned to this release's
/// `models/index.json`; upgrade them together.
pub const NEMO_SPEECH_VERSION: &str = "0.2.0";
