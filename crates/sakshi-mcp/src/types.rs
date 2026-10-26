//! Tool parameters and structured results.
//!
//! Optional fields are declared with a single JSON type and left out when
//! absent (`#[schemars(with = ...)]` plus `serde(default)` or
//! `skip_serializing_if`), instead of the `["type", "null"]` arrays that
//! clients mapping schemas onto single-type dialects reject.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListMeetingsParams {
    /// Only meetings that started at or after this time: RFC 3339 (e.g.
    /// "2026-10-26T09:00:00Z") or a UTC date ("2026-10-26").
    #[serde(default)]
    #[schemars(with = "String")]
    pub since: Option<String>,
    /// Only meetings that started before this time; same formats as `since`.
    #[serde(default)]
    #[schemars(with = "String")]
    pub until: Option<String>,
    /// Text that must appear in the meeting title or transcript.
    #[serde(default)]
    #[schemars(with = "String")]
    pub query: Option<String>,
    /// Maximum number of meetings (1-100, default 20).
    #[serde(default)]
    #[schemars(with = "u32")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MeetingIdParams {
    /// A meeting id from `list_meetings`.
    pub meeting_id: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptFormat {
    /// Structured segments.
    #[default]
    Json,
    /// One line per segment: "[m:ss] Speaker: text".
    Text,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTranscriptParams {
    /// A meeting id from `list_meetings`.
    pub meeting_id: String,
    /// Only segments overlapping this window, in milliseconds from the start.
    #[serde(default)]
    #[schemars(with = "u64")]
    pub start_ms: Option<u64>,
    /// End of the window (exclusive), in milliseconds from the start.
    #[serde(default)]
    #[schemars(with = "u64")]
    pub end_ms: Option<u64>,
    /// "json" (default) or "text".
    #[serde(default)]
    #[schemars(with = "TranscriptFormat")]
    pub format: Option<TranscriptFormat>,
    /// Include per-word timing in JSON output (larger responses).
    #[serde(default)]
    #[schemars(with = "bool")]
    pub include_words: Option<bool>,
    /// The `next_cursor` from the previous page, to continue reading.
    #[serde(default)]
    #[schemars(with = "String")]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFramesParams {
    /// A meeting id from `list_meetings`.
    pub meeting_id: String,
    /// Start of the window, in milliseconds from the start of the recording.
    #[serde(default)]
    #[schemars(with = "u64")]
    pub start_ms: Option<u64>,
    /// End of the window (exclusive), in milliseconds.
    #[serde(default)]
    #[schemars(with = "u64")]
    pub end_ms: Option<u64>,
    /// Maximum frames to return (1-16, default 8). When the window holds
    /// more, frames are picked evenly across it.
    #[serde(default)]
    #[schemars(with = "u32")]
    pub max: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFrameAtParams {
    /// A meeting id from `list_meetings`.
    pub meeting_id: String,
    /// A moment in the meeting, in milliseconds from the start.
    pub at_ms: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Text to find in transcripts (case-insensitive substring).
    pub query: String,
    /// Maximum results (1-100, default 20).
    #[serde(default)]
    #[schemars(with = "u32")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveSummaryParams {
    /// The meeting this summary describes.
    pub meeting_id: String,
    /// The summary, in Markdown (up to 256 KiB).
    pub markdown: String,
    /// A one-line title.
    #[serde(default)]
    #[schemars(with = "String")]
    pub title: Option<String>,
    /// The model that wrote the summary, e.g. "claude-opus-5-5".
    #[serde(default)]
    #[schemars(with = "String")]
    pub author_model: Option<String>,
}

/// A meeting as listed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeetingOut {
    pub id: String,
    pub title: String,
    /// RFC 3339 start time; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub started_at: Option<String>,
    /// RFC 3339 time Sakshi last processed the meeting.
    pub processed_at: String,
    pub duration_ms: u64,
    /// "You" and "Speaker N" labels in order of first appearance.
    pub speakers: Vec<String>,
    /// Languages the recognizer reported, if any.
    pub languages: Vec<String>,
    pub segment_count: u32,
    pub frame_count: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MeetingList {
    pub meetings: Vec<MeetingOut>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummaryInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub author_model: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MeetingDetails {
    pub meeting: MeetingOut,
    /// Summaries saved for this meeting; read them with `list_summaries`.
    pub summaries: Vec<SummaryInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WordOut {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SegmentOut {
    pub id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    /// "mic" (the user) or "system" (everyone else).
    pub track: String,
    /// "You" or "Speaker N".
    pub speaker: String,
    pub text: String,
    /// The key frame on screen when the segment started (pass it to
    /// `get_frame_at` as `at_ms`); absent when the meeting has no video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u64")]
    pub frame_id: Option<u64>,
    /// Per-word timing, when `include_words` was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<WordOut>")]
    pub words: Option<Vec<WordOut>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TranscriptPage {
    pub meeting_id: String,
    pub segments: Vec<SegmentOut>,
    /// Key frames on screen during the returned segments.
    pub frame_ids: Vec<u64>,
    /// Pass as `cursor` to read the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHitOut {
    pub meeting_id: String,
    pub meeting_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub started_at: Option<String>,
    pub segment_id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    pub results: Vec<SearchHitOut>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SavedSummary {
    pub summary_id: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummaryOut {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub author_model: Option<String>,
    pub created_at: String,
    /// Absent when the response size limit was reached; see `truncated`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub markdown: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SummaryList {
    pub summaries: Vec<SummaryOut>,
    /// True if some summaries' Markdown was left out to keep the response
    /// small.
    pub truncated: bool,
}
