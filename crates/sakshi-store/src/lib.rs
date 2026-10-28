//! SQLite index of processed meetings: metadata, transcript segments, key
//! frames, summaries, and full-text search.
//!
//! The meeting folders are the source of truth; the database is an index
//! that can be rebuilt from them. [`Store`] opened read-write indexes
//! folders; opened read-only it answers queries. [`SummaryWriter`] can only
//! save summaries and [`SpeakerNameWriter`] only name speakers: the two
//! writes the MCP server is allowed to make to the index.

mod error;
mod schema;
mod speakers;
mod store;
mod summaries;

pub use error::StoreError;
pub use speakers::SpeakerNameWriter;
pub use store::{MeetingQuery, MeetingRecord, SearchHit, Store, TimeRange};
pub use summaries::SummaryWriter;
