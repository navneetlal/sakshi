//! What the CLI and the desktop app both do: process a meeting end to end,
//! create a folder for a new recording, read settings, and set up MCP
//! clients. Front ends only add their own way of showing progress.

mod error;
pub mod mcp_setup;
mod processing;
mod recording;
mod settings;

pub use error::AppError;
pub use processing::{ProcessOptions, ProcessOutcome, Progress, process_meeting};
pub use recording::create_meeting;
pub use settings::{
    check_language, load_settings, resolve_language, save_settings, set_meeting_language,
    supported_languages,
};
