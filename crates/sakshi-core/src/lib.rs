//! Domain types shared by every Sakshi crate: meetings, tracks, transcript
//! segments, frames, and the on-disk data layout.

mod error;
mod import;
mod json;
mod language;
mod layout;
mod meeting;
mod transcript;

pub use error::Error;
pub use import::{ImportRequest, ImportState};
pub use json::{read_json, write_json_atomic};
pub use language::Language;
pub use layout::{DataDir, MeetingDir};
pub use meeting::{Frame, MeetingInfo, Settings, SpeakerNames, Summary, VideoPause, WindowView};
pub use transcript::{Segment, Speaker, Track, Transcript, Word};
