use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error(
        "ffmpeg not found. Install FFmpeg 8.0 or newer (e.g. `brew install ffmpeg`) or set {env} to the binary's path"
    )]
    FfmpegNotFound { env: &'static str },

    #[error("{} doesn't look like ffmpeg: `-version` printed no version", path.display())]
    NotFfmpeg { path: PathBuf },

    #[error("running {}: {source}", path.display())]
    Spawn { path: PathBuf, source: io::Error },

    #[error("ffmpeg failed ({status}) on {}:\n{stderr}", input.display())]
    FfmpegFailed {
        input: PathBuf,
        status: String,
        stderr: String,
    },

    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("{}: not a readable WAV file: {source}", path.display())]
    Wav { path: PathBuf, source: hound::Error },

    #[error("unexpected frame data from ffmpeg: {0}")]
    FrameStream(String),

    #[error("writing {}: {source}", path.display())]
    Jpeg {
        path: PathBuf,
        source: image::ImageError,
    },
}

impl MediaError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
