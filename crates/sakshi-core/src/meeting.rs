use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Language};

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
    /// Names the user gave the meeting's speakers.
    pub speaker_names: SpeakerNames,
    /// The file a recording made elsewhere (Zoom, Fireflies, ...) was
    /// imported from: its name only, never its path.
    pub imported_from: Option<String>,
}

/// Names for a meeting's speakers, keyed by label ("Speaker 1", "You").
/// Sakshi never guesses names: these come from the user, or a client acting
/// for them. A label without a name shows as itself, and two labels may
/// share a name when diarization split one person in two.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpeakerNames(BTreeMap<String, String>);

impl SpeakerNames {
    /// Longest name accepted, in characters.
    pub const MAX_CHARS: usize = 80;

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, label: &str) -> Option<&str> {
        self.0.get(label).map(String::as_str)
    }

    /// The name given to `label`, or the label itself.
    pub fn display<'a>(&'a self, label: &'a str) -> &'a str {
        self.get(label).unwrap_or(label)
    }

    /// `(label, name)` pairs in label order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(l, n)| (l.as_str(), n.as_str()))
    }

    /// Names `label`, or removes its name when `name` is `None` or blank.
    pub fn set(&mut self, label: &str, name: Option<&str>) -> Result<(), Error> {
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            None => {
                self.0.remove(label);
            }
            Some(n) if n.chars().count() > Self::MAX_CHARS || n.contains(['\n', '\r']) => {
                return Err(Error::InvalidSpeakerName(n.to_owned()));
            }
            Some(n) => {
                self.0.insert(label.to_owned(), n.to_owned());
            }
        }
        Ok(())
    }
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

/// How much of a recorded window could be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowView {
    /// At least part of it was on screen.
    Visible,
    /// On screen, but entirely behind other windows.
    Covered,
    /// Minimized, or on a desktop that wasn't showing.
    Hidden,
}

/// A stretch of a recording in which the recorded window was out of view
/// and its app sent little or no new video, so the recording holds an old
/// picture there. Audio is complete through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoPause {
    /// Milliseconds from the start of the recording.
    pub from_ms: u64,
    pub to_ms: u64,
    /// Where the window was when the pause ended.
    pub window: WindowView,
}

impl VideoPause {
    pub fn contains(&self, at_ms: u64) -> bool {
        (self.from_ms..self.to_ms).contains(&at_ms)
    }

    /// Whether the pause overlaps `[start_ms, end_ms)`.
    pub fn overlaps(&self, start_ms: u64, end_ms: u64) -> bool {
        self.from_ms < end_ms && start_ms < self.to_ms
    }
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
    fn video_pauses_serialize_in_snake_case_and_span_half_open() {
        let pause = VideoPause {
            from_ms: 1_000,
            to_ms: 2_000,
            window: WindowView::Covered,
        };
        let json = serde_json::to_string(&pause).unwrap();
        assert_eq!(json, r#"{"from_ms":1000,"to_ms":2000,"window":"covered"}"#);
        assert!(pause.contains(1_000) && pause.contains(1_999) && !pause.contains(2_000));
        assert!(pause.overlaps(0, 1_001) && pause.overlaps(1_999, 5_000));
        assert!(!pause.overlaps(0, 1_000) && !pause.overlaps(2_000, 3_000));
    }

    #[test]
    fn speaker_names_set_clear_and_fall_back_to_the_label() {
        let mut names = SpeakerNames::default();
        names.set("Speaker 1", Some("  Priya ")).unwrap();
        names.set("Speaker 2", Some("Priya")).unwrap();
        assert_eq!(names.display("Speaker 1"), "Priya");
        assert_eq!(names.display("Speaker 3"), "Speaker 3");

        names.set("Speaker 2", Some("   ")).unwrap();
        names.set("Speaker 9", None).unwrap();
        assert_eq!(names.iter().collect::<Vec<_>>(), [("Speaker 1", "Priya")]);

        assert!(names.set("You", Some("a\nb")).is_err());
        assert!(names.set("You", Some(&"x".repeat(81))).is_err());
        assert_eq!(names.get("You"), None);
    }

    #[test]
    fn older_meeting_json_has_no_names_and_names_round_trip() {
        let info: MeetingInfo = serde_json::from_str(r#"{"title": "Standup"}"#).unwrap();
        assert!(info.speaker_names.is_empty());
        assert_eq!(info.imported_from, None);

        let json = r#"{"speaker_names": {"Speaker 1": "Priya"}, "imported_from": "call.mp4"}"#;
        let info: MeetingInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.speaker_names.display("Speaker 1"), "Priya");
        let back: MeetingInfo =
            serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn meeting_language_parses() {
        let info: MeetingInfo = serde_json::from_str(r#"{"language": "hi"}"#).unwrap();
        assert_eq!(info.language, Some(Language::Code("hi".into())));
    }
}
