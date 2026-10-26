use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Core(#[from] sakshi_core::Error),

    #[error(transparent)]
    Asr(#[from] sakshi_asr::AsrError),

    #[error(transparent)]
    Media(#[from] sakshi_media::MediaError),

    #[error("{}: no mic.wav or system.wav to process", dir.display())]
    NoAudio { dir: PathBuf },

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl PipelineError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
