//! SQLite index of processed meetings: metadata, transcript segments, key
//! frames, summaries, and full-text search.
//!
//! The meeting folders are the source of truth; the database is an index
//! that can be rebuilt from them. [`Store`] opened read-write indexes
//! folders; opened read-only it answers queries. [`SummaryWriter`] can only
//! save summaries, which is all the MCP server is allowed to write.

mod error;
mod schema;
mod store;
mod summaries;

pub use error::StoreError;
pub use store::{MeetingQuery, MeetingRecord, SearchHit, Store, TimeRange};
pub use summaries::SummaryWriter;
