use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{DataDir, Error, Language, read_json, write_json_atomic};

/// A recording made elsewhere (Zoom, Fireflies, ...) waiting to become a
/// meeting, kept as `imports/<meeting_id>.json` until it has been
/// processed. The MCP server or the app writes it; the app picks it up,
/// copies the recording into `meetings/<meeting_id>/`, and processes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportRequest {
    /// The id the meeting will have.
    pub id: String,
    /// The recording, as an absolute path on this computer.
    pub source: PathBuf,
    pub title: Option<String>,
    /// RFC 3339 start time, if the requester knows it.
    pub started_at: Option<String>,
    pub language: Option<Language>,
    /// RFC 3339.
    pub requested_at: String,
    #[serde(flatten)]
    pub state: ImportState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ImportState {
    Queued,
    /// `step` is what is running: copying the recording, or a pipeline
    /// stage.
    Processing {
        step: String,
    },
    Failed {
        error: String,
    },
}

impl ImportRequest {
    /// Checks that `source` is a readable file and returns its absolute
    /// path. The import itself finds out whether it holds audio.
    pub fn check_source(source: &Path) -> Result<PathBuf, Error> {
        if !source.is_absolute() {
            return Err(Error::InvalidImport(format!(
                "{} is not an absolute path",
                source.display()
            )));
        }
        let meta = std::fs::metadata(source).map_err(|e| Error::io(source, e))?;
        if !meta.is_file() {
            return Err(Error::InvalidImport(format!(
                "{} is not a file",
                source.display()
            )));
        }
        std::fs::File::open(source).map_err(|e| Error::io(source, e))?;
        Ok(source.to_path_buf())
    }

    /// The pending request for a meeting, if there is one.
    pub fn load(data: &DataDir, meeting_id: &str) -> Result<Option<Self>, Error> {
        let path = data.import_request(meeting_id);
        if !path.is_file() {
            return Ok(None);
        }
        read_json(&path).map(Some)
    }

    /// Every pending request, oldest first. Unreadable files are skipped.
    pub fn all(data: &DataDir) -> Result<Vec<Self>, Error> {
        let dir = data.imports_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(&dir, e)),
        };
        let mut requests: Vec<Self> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| read_json(&p).ok())
            .collect();
        requests.sort_by(|a, b| a.requested_at.cmp(&b.requested_at).then(a.id.cmp(&b.id)));
        Ok(requests)
    }

    pub fn save(&self, data: &DataDir) -> Result<(), Error> {
        write_json_atomic(&data.import_request(&self.id), self)
    }

    /// Removes the request once the meeting has been processed.
    pub fn remove(data: &DataDir, meeting_id: &str) -> Result<(), Error> {
        let path = data.import_request(meeting_id);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(&path, e)),
            _ => Ok(()),
        }
    }

    /// The source's file name, which is all of its path the meeting keeps.
    pub fn file_name(&self) -> String {
        self.source
            .file_name()
            .map_or_else(|| "recording".into(), |n| n.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, requested_at: &str) -> ImportRequest {
        ImportRequest {
            id: id.into(),
            source: "/tmp/call.mp4".into(),
            title: None,
            started_at: None,
            language: None,
            requested_at: requested_at.into(),
            state: ImportState::Queued,
        }
    }

    #[test]
    fn saves_lists_and_removes_requests() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path());
        assert!(
            ImportRequest::all(&data).unwrap().is_empty(),
            "no folder yet"
        );

        request("b", "2026-10-28T10:00:00Z").save(&data).unwrap();
        let mut failed = request("a", "2026-10-28T11:00:00Z");
        failed.state = ImportState::Failed {
            error: "no audio".into(),
        };
        failed.save(&data).unwrap();
        std::fs::write(data.imports_dir().join("junk.json"), "{").unwrap();

        let all = ImportRequest::all(&data).unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(ImportRequest::load(&data, "a").unwrap(), Some(failed));
        let json = std::fs::read_to_string(data.import_request("a")).unwrap();
        assert!(json.contains(r#""state": "failed""#), "{json}");

        ImportRequest::remove(&data, "a").unwrap();
        ImportRequest::remove(&data, "a").unwrap();
        assert_eq!(ImportRequest::load(&data, "a").unwrap(), None);
        assert_eq!(request("b", "").file_name(), "call.mp4");
    }

    #[test]
    fn checks_the_source() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("call.mp4");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(ImportRequest::check_source(&file).unwrap(), file);
        assert!(ImportRequest::check_source(Path::new("call.mp4")).is_err());
        assert!(ImportRequest::check_source(root.path()).is_err());
        assert!(ImportRequest::check_source(&root.path().join("gone.mp4")).is_err());
    }
}
