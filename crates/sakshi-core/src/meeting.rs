use serde::{Deserialize, Serialize};

use crate::Language;

/// Contents of a meeting's `meeting.json`. Every field is optional, so a
/// bare folder of recordings is a valid meeting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingInfo {
    pub title: Option<String>,
    /// RFC 3339 start time of the recording.
    pub started_at: Option<String>,
    /// Pins this meeting's language, overriding [`Settings::language`].
    pub language: Option<Language>,
}

/// Global settings, stored as `config.json` in the data directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Default transcription language for every meeting.
    pub language: Language,
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
    }

    #[test]
    fn meeting_language_parses() {
        let info: MeetingInfo = serde_json::from_str(r#"{"language": "hi"}"#).unwrap();
        assert_eq!(info.language, Some(Language::Code("hi".into())));
    }
}
