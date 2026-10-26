//! Resumable post-recording pipeline that turns `mic.wav`, `system.wav`,
//! and `recording.mp4` into `transcript.json` and key frames.
//!
//! The pipeline records and labels; it never summarizes or interprets.

mod bleed;
mod error;
mod pipeline;
mod segments;
mod state;
mod text;

pub use bleed::{BleedConfig, BleedMatch, remove_bleed};
pub use error::PipelineError;
pub use pipeline::{Observer, Pipeline, PipelineConfig, Report, RunOptions, StageEvent};
pub use segments::build_segments;
pub use state::{PipelineState, Stage, StageRecord};
