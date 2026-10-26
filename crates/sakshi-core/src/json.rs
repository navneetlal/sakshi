use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Error;

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, Error> {
    let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes pretty JSON to a sibling temp file, syncs it, and renames it over
/// `path`, so a crash leaves either the old file or the new one.
pub fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);

    let file = File::create(&tmp).map_err(|e| Error::io(&tmp, e))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value).map_err(|source| Error::Json {
        path: tmp.clone(),
        source,
    })?;
    writer.write_all(b"\n").map_err(|e| Error::io(&tmp, e))?;
    let file = writer
        .into_inner()
        .map_err(|e| Error::io(&tmp, e.into_error()))?;
    file.sync_all().map_err(|e| Error::io(&tmp, e))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn round_trips_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("state.json");
        let value = BTreeMap::from([("stage", "done")]);

        write_json_atomic(&path, &value).unwrap();
        let back: BTreeMap<String, String> = read_json(&path).unwrap();

        assert_eq!(back["stage"], "done");
        assert!(!dir.path().join("nested").join("state.json.tmp").exists());
    }

    #[test]
    fn reports_path_on_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{not json").unwrap();

        let err = read_json::<BTreeMap<String, String>>(&path).unwrap_err();

        assert!(err.to_string().contains("bad.json"), "{err}");
    }
}
