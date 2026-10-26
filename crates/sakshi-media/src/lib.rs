//! FFmpeg sidecar integration: resampling audio to 16 kHz mono and
//! extracting key frames from the recorded video.

mod audio;
mod error;
mod ffmpeg;
mod keyframes;

pub use audio::{WavInfo, wav_info};
pub use error::MediaError;
pub use ffmpeg::{FFMPEG_ENV, Ffmpeg};
pub use keyframes::{KeyframeConfig, KeyframeSelector, hamming};
