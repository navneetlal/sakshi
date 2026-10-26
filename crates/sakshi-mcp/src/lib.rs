//! MCP server for Sakshi: lets AI clients list meetings, read transcripts,
//! look at key frames, search, and save summaries they wrote.
//!
//! The server reads the same SQLite index and meeting folders as the CLI, so
//! it works whether or not the desktop app is running. It only ever writes
//! summaries.

mod format;
mod http;
mod server;
mod types;

pub use http::{generate_token, http_router};
pub use server::{INSTRUCTIONS, SakshiMcp};
pub use types::*;
