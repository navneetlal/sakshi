use std::io;
use std::path::PathBuf;

use crate::gguf::GgufError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] sakshi_core::Error),

    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("Hugging Face: {0}")]
    Hub(#[from] hf_hub::HFError),

    #[error("Hugging Face API: {0}")]
    Api(#[from] reqwest::Error),

    #[error("bundled model manifest is invalid: {0}")]
    InvalidManifest(String),

    #[error("unknown model {0:?}; run `sakshi-cli models list` to see available models")]
    UnknownModel(String),

    #[error("model {id} is not downloaded; run `sakshi-cli models pull {id}`")]
    NotInstalled { id: String },

    #[error("the license for {id} ({license}) must be accepted before downloading")]
    LicenseNotAccepted { id: String, license: String },

    #[error("{}: size is {actual} bytes, expected {expected}", path.display())]
    SizeMismatch {
        path: PathBuf,
        expected: u64,
        actual: u64,
    },

    #[error("{}: SHA-256 is {actual}, expected {expected}", path.display())]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("invalid repository id {0:?}; expected owner/name")]
    InvalidRepoId(String),

    #[error("{repo} has no file {filename:?} at revision {revision}")]
    FileNotFound {
        repo: String,
        filename: String,
        revision: String,
    },

    #[error(
        "{filename}: {reason}. Sakshi only runs GGUF models that the nemo-speech runtime can load \
         (Nemotron ASR or Sortformer diarization)"
    )]
    UnsupportedModel { filename: String, reason: String },

    #[error("{filename}: {source}")]
    Gguf { filename: String, source: GgufError },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
