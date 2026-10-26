use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("{}: invalid JSON: {source}", path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error(
        "invalid language {0:?}: use \"auto\" or a language code such as \"en\", \"hi\", or \"en-US\""
    )]
    InvalidLanguage(String),

    #[error("invalid speaker label {0:?}: expected \"You\" or \"Speaker N\" with N >= 1")]
    InvalidSpeaker(String),

    #[error("no platform data directory found; set SAKSHI_DATA_DIR")]
    NoDataDir,
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
