//! Which pipeline stages have finished, persisted as `pipeline.json`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use sakshi_core::{read_json, write_json_atomic};
use serde::{Deserialize, Serialize};

use crate::PipelineError;

/// Pipeline stages in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Make sure both audio tracks are 16 kHz mono.
    PrepareAudio,
    /// Transcribe and diarize the system track.
    TranscribeSystem,
    /// Transcribe the mic track.
    TranscribeMic,
    /// Build speaker-labelled segments from the words.
    Segments,
    /// Drop mic segments that repeat the system track.
    RemoveBleed,
    /// Extract key frames from the video.
    Keyframes,
    /// Write `transcript.json`.
    Save,
}

impl Stage {
    pub const ALL: [Stage; 7] = [
        Stage::PrepareAudio,
        Stage::TranscribeSystem,
        Stage::TranscribeMic,
        Stage::Segments,
        Stage::RemoveBleed,
        Stage::Keyframes,
        Stage::Save,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::PrepareAudio => "prepare-audio",
            Stage::TranscribeSystem => "transcribe-system",
            Stage::TranscribeMic => "transcribe-mic",
            Stage::Segments => "segments",
            Stage::RemoveBleed => "remove-bleed",
            Stage::Keyframes => "keyframes",
            Stage::Save => "save",
        }
    }

    /// Stages whose output this stage reads. If one of them runs again,
    /// this stage must too.
    pub fn inputs(self) -> &'static [Stage] {
        match self {
            Stage::PrepareAudio | Stage::Keyframes => &[],
            Stage::TranscribeSystem | Stage::TranscribeMic => &[Stage::PrepareAudio],
            Stage::Segments => &[Stage::TranscribeSystem, Stage::TranscribeMic],
            Stage::RemoveBleed => &[Stage::Segments],
            Stage::Save => &[Stage::RemoveBleed, Stage::Keyframes],
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Stage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Stage::ALL
            .into_iter()
            .find(|stage| stage.as_str() == s)
            .ok_or_else(|| {
                let names: Vec<_> = Stage::ALL.iter().map(|s| s.as_str()).collect();
                format!("unknown stage {s:?}; stages are {}", names.join(", "))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageRecord {
    pub completed_at_unix_s: u64,
    /// The settings the stage ran with. A different key means the stage's
    /// output is stale.
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineState {
    pub schema_version: u32,
    pub stages: BTreeMap<String, StageRecord>,
}

impl PipelineState {
    const SCHEMA_VERSION: u32 = 1;

    pub fn load(path: &Path) -> Result<Self, PipelineError> {
        if !path.exists() {
            return Ok(Self {
                schema_version: Self::SCHEMA_VERSION,
                ..Self::default()
            });
        }
        Ok(read_json(path)?)
    }

    pub fn save(&self, path: &Path) -> Result<(), PipelineError> {
        Ok(write_json_atomic(path, self)?)
    }

    pub fn get(&self, stage: Stage) -> Option<&StageRecord> {
        self.stages.get(stage.as_str())
    }

    pub fn record(&mut self, stage: Stage, key: String, note: Option<String>) {
        let completed_at_unix_s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.stages.insert(
            stage.as_str().to_owned(),
            StageRecord {
                completed_at_unix_s,
                key,
                note,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_round_trip() {
        for stage in Stage::ALL {
            assert_eq!(stage.as_str().parse::<Stage>().unwrap(), stage);
        }
        assert!("transcribe".parse::<Stage>().is_err());
    }

    #[test]
    fn inputs_come_earlier() {
        for stage in Stage::ALL {
            assert!(stage.inputs().iter().all(|&input| input < stage), "{stage}");
        }
    }

    #[test]
    fn state_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipeline.json");
        let mut state = PipelineState::load(&path).unwrap();
        state.record(Stage::Segments, "gap=1000".into(), None);
        state.save(&path).unwrap();

        let back = PipelineState::load(&path).unwrap();
        assert_eq!(back.get(Stage::Segments).unwrap().key, "gap=1000");
        assert_eq!(back.schema_version, 1);
    }
}
