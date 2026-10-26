use std::path::{Path, PathBuf};

use crate::Error;

/// Environment variable that overrides the data directory.
pub const DATA_DIR_ENV: &str = "SAKSHI_DATA_DIR";

/// The Sakshi data directory:
///
/// ```text
/// <app data dir>/Sakshi/
///   sakshi.db
///   config.json
///   models/
///   meetings/<meeting_id>/
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$SAKSHI_DATA_DIR` if set, otherwise `Sakshi/` in the platform data
    /// directory (`~/Library/Application Support` on macOS, `%APPDATA%` on
    /// Windows, `$XDG_DATA_HOME` or `~/.local/share` on Linux).
    pub fn from_env_or_default() -> Result<Self, Error> {
        if let Some(root) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::new(root));
        }
        dirs::data_dir()
            .map(|d| Self::new(d.join("Sakshi")))
            .ok_or(Error::NoDataDir)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join("sakshi.db")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn meetings_dir(&self) -> PathBuf {
        self.root.join("meetings")
    }

    pub fn meeting(&self, meeting_id: &str) -> MeetingDir {
        MeetingDir::new(self.meetings_dir().join(meeting_id))
    }
}

/// One meeting's folder:
///
/// ```text
/// recording.mp4   mic.wav   system.wav   meeting.json
/// frames/<ms>.jpg   frames.json   transcript.json   summaries/<id>.md
/// pipeline.json   work/
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingDir {
    dir: PathBuf,
}

impl MeetingDir {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn recording_mp4(&self) -> PathBuf {
        self.dir.join("recording.mp4")
    }

    pub fn mic_wav(&self) -> PathBuf {
        self.dir.join("mic.wav")
    }

    pub fn system_wav(&self) -> PathBuf {
        self.dir.join("system.wav")
    }

    pub fn info_json(&self) -> PathBuf {
        self.dir.join("meeting.json")
    }

    pub fn frames_dir(&self) -> PathBuf {
        self.dir.join("frames")
    }

    /// The file name of a frame relative to the meeting folder.
    pub fn frame_file(at_ms: u64) -> String {
        format!("frames/{at_ms}.jpg")
    }

    pub fn frames_json(&self) -> PathBuf {
        self.dir.join("frames.json")
    }

    pub fn transcript_json(&self) -> PathBuf {
        self.dir.join("transcript.json")
    }

    pub fn summaries_dir(&self) -> PathBuf {
        self.dir.join("summaries")
    }

    /// Records which pipeline stages have finished.
    pub fn pipeline_json(&self) -> PathBuf {
        self.dir.join("pipeline.json")
    }

    /// Intermediate pipeline outputs.
    pub fn work_dir(&self) -> PathBuf {
        self.dir.join("work")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_documented_tree() {
        let data = DataDir::new("/data/Sakshi");
        let meeting = data.meeting("m1");

        assert_eq!(data.db_path(), Path::new("/data/Sakshi/sakshi.db"));
        assert_eq!(data.models_dir(), Path::new("/data/Sakshi/models"));
        assert_eq!(meeting.path(), Path::new("/data/Sakshi/meetings/m1"));
        assert_eq!(
            meeting.transcript_json(),
            Path::new("/data/Sakshi/meetings/m1/transcript.json")
        );
        assert_eq!(MeetingDir::frame_file(12_000), "frames/12000.jpg");
    }
}
