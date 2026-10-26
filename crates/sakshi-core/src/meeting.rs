use serde::{Deserialize, Serialize};

use crate::Language;

/// Contents of a meeting's `meeting.json`. Every field is optional, so a
/// bare folder of recordings is a valid meeting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingInfo {
    /// Assigned when the meeting is first indexed, so the folder keeps its
    /// identity if it moves.
    pub id: Option<String>,
    pub title: Option<String>,
    /// RFC 3339 start time of the recording.
    pub started_at: Option<String>,
    /// Pins this meeting's language, overriding [`Settings::language`].
    pub language: Option<Language>,
}

/// Global settings, stored as `config.json` in the data directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Default transcription language for every meeting.
    pub language: Language,
    /// Remind the user to tell participants before each recording.
    pub consent_reminder: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            consent_reminder: true,
        }
    }
}

/// A summary written by an MCP client and saved through Sakshi. Sakshi
/// stores summaries; it never writes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub meeting_id: String,
    pub title: Option<String>,
    /// The model that wrote it, as the client reported.
    pub author_model: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    pub markdown: String,
}

/// A key frame extracted from `recording.mp4`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Time of the frame, in milliseconds from the start of the recording.
    pub at_ms: u64,
    /// Path relative to the meeting folder, e.g. `frames/12000.jpg`.
    pub file: String,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default() {
        let info: MeetingInfo = serde_json::from_str("{}").unwrap();
        assert_eq!(info, MeetingInfo::default());
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.language, Language::Auto);
        assert!(settings.consent_reminder, "on unless turned off");
        let settings: Settings =
            serde_json::from_str(r#"{"language": "hi", "consent_reminder": false}"#).unwrap();
        assert!(!settings.consent_reminder);
    }

    #[test]
    fn meeting_language_parses() {
        let info: MeetingInfo = serde_json::from_str(r#"{"language": "hi"}"#).unwrap();
        assert_eq!(info.language, Some(Language::Code("hi".into())));
    }
}
