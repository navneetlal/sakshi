use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Core(#[from] sakshi_core::Error),

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{}: not processed yet (no transcript.json); run `sakshi-cli process` first", dir.display())]
    NotProcessed { dir: PathBuf },

    #[error(
        "{}: no Sakshi database yet; process a meeting with `sakshi-cli process` first",
        path.display()
    )]
    NoDatabase { path: PathBuf },

    #[error(
        "the database was written by a newer Sakshi (schema {found}, this build knows {known})"
    )]
    NewerSchema { found: i64, known: i64 },

    #[error(
        "the database schema is {found}, expected {known}; run `sakshi-cli` once to upgrade it"
    )]
    OutdatedSchema { found: i64, known: i64 },

    #[error("no meeting with id {0}")]
    UnknownMeeting(String),

    #[error("invalid summary: {0}")]
    InvalidSummary(String),
}

impl StoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
