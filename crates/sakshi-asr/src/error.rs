use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error(
        "nemo-speech {expected} not found. Run `cargo xtask fetch-sidecar` in a development \
         checkout, or set {env} to the binary's path"
    )]
    SidecarNotFound { expected: String, env: &'static str },

    #[error("{} is nemo-speech {found}, but Sakshi requires {expected}", path.display())]
    SidecarVersion {
        path: PathBuf,
        found: String,
        expected: String,
    },

    #[error("running {}: {source}", path.display())]
    Spawn { path: PathBuf, source: io::Error },

    #[error("nemo-speech failed ({status}):\n{stderr}")]
    SidecarFailed { status: String, stderr: String },

    #[error(
        "nemo-speech ran with unexpected settings ({line}); expected streaming with right context {expected}"
    )]
    UnexpectedGeometry { line: String, expected: u32 },

    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("{}: unexpected nemo-speech output: {source}", path.display())]
    Output {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("reading {}: {source}", path.display())]
    ModelHeader {
        path: PathBuf,
        source: sakshi_models::gguf::GgufError,
    },

    #[error("{0}")]
    UnsupportedLanguage(String),

    #[error("diarization was requested but no diarization model is configured")]
    NoDiarizationModel,
}
