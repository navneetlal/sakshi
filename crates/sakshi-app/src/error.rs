use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Core(#[from] sakshi_core::Error),

    #[error(transparent)]
    Models(#[from] sakshi_models::Error),

    #[error(transparent)]
    Asr(#[from] sakshi_asr::AsrError),

    #[error(transparent)]
    Media(#[from] sakshi_media::MediaError),

    #[error(transparent)]
    Pipeline(#[from] sakshi_pipeline::PipelineError),

    #[error(transparent)]
    Store(#[from] sakshi_store::StoreError),

    #[error(transparent)]
    Capture(#[from] sakshi_capture::CaptureError),

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{0}")]
    Invalid(String),
}

impl AppError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
