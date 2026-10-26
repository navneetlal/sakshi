use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// Which recorded audio stream a segment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Track {
    /// The local microphone: always the user ("You").
    Mic,
    /// Meeting-app or system audio: everyone else.
    System,
}

impl Track {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
        }
    }
}

impl fmt::Display for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A generic speaker label. Sakshi never guesses names: remote speakers are
/// numbered in order of first appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Speaker {
    /// The local user, speaking into the mic track.
    You,
    /// A remote participant, `Speaker 1` and up.
    Remote(u8),
}

impl fmt::Display for Speaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::You => f.write_str("You"),
            Self::Remote(n) => write!(f, "Speaker {n}"),
        }
    }
}

impl FromStr for Speaker {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        if s == "You" {
            return Ok(Self::You);
        }
        s.strip_prefix("Speaker ")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|&n| n >= 1)
            .map(Self::Remote)
            .ok_or_else(|| Error::InvalidSpeaker(s.to_owned()))
    }
}

impl TryFrom<String> for Speaker {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Error> {
        s.parse()
    }
}

impl From<Speaker> for String {
    fn from(speaker: Speaker) -> Self {
        speaker.to_string()
    }
}

/// One recognized word. Times are milliseconds from the start of the
/// recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Recognizer confidence in `0..=1`, as reported by the engine.
    pub p: f32,
}

/// A run of words from one speaker on one track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub track: Track,
    pub speaker: Speaker,
    pub text: String,
    pub words: Vec<Word>,
}

/// The contents of `transcript.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub schema_version: u32,
    /// Length of the longest audio track.
    #[serde(default)]
    pub duration_ms: u64,
    /// Languages the recognizer reported, e.g. `["en-US"]`.
    pub languages: Vec<String>,
    /// Segments from both tracks, ordered by start time.
    pub segments: Vec<Segment>,
}

impl Transcript {
    pub const SCHEMA_VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_labels_round_trip() {
        assert_eq!("You".parse::<Speaker>().unwrap(), Speaker::You);
        assert_eq!("Speaker 3".parse::<Speaker>().unwrap(), Speaker::Remote(3));
        assert_eq!(Speaker::Remote(8).to_string(), "Speaker 8");
        for bad in ["you", "Speaker 0", "Speaker", "Speaker x", "Speaker 300"] {
            assert!(bad.parse::<Speaker>().is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn segment_serializes_to_the_documented_shape() {
        let segment = Segment {
            id: 7,
            start_ms: 1200,
            end_ms: 1900,
            track: Track::System,
            speaker: Speaker::Remote(2),
            text: "Hello there".into(),
            words: vec![Word {
                start_ms: 1200,
                end_ms: 1500,
                text: "Hello".into(),
                p: 1.0,
            }],
        };

        let json = serde_json::to_value(&segment).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "id": 7,
                "start_ms": 1200,
                "end_ms": 1900,
                "track": "system",
                "speaker": "Speaker 2",
                "text": "Hello there",
                "words": [{"start_ms": 1200, "end_ms": 1500, "text": "Hello", "p": 1.0}]
            })
        );
        let back: Segment = serde_json::from_value(json).unwrap();
        assert_eq!(back, segment);
    }
}
