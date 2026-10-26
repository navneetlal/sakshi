use std::path::{Path, PathBuf};

use sakshi_core::{read_json, write_json_atomic};

use crate::{Error, Manifest, ModelEntry, Role};

/// The local models folder:
///
/// ```text
/// models/
///   <owner>/<name>/<revision>/<filename>
///   custom.json          models added by the user
///   .partial/            downloads in progress
/// ```
#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
    manifest: Manifest,
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>, manifest: Manifest) -> Self {
        Self {
            dir: dir.into(),
            manifest,
        }
    }

    /// The store for a data directory, using the bundled manifest.
    pub fn open(data: &sakshi_core::DataDir) -> Result<Self, Error> {
        Ok(Self::new(data.models_dir(), Manifest::bundled()?))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bundled models followed by custom ones.
    pub fn entries(&self) -> Result<Vec<ModelEntry>, Error> {
        let mut all = self.manifest.models.clone();
        all.extend(self.custom_entries()?);
        Ok(all)
    }

    pub fn find(&self, id: &str) -> Result<ModelEntry, Error> {
        self.entries()?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| Error::UnknownModel(id.to_owned()))
    }

    pub fn default_for(&self, role: Role) -> Result<ModelEntry, Error> {
        self.manifest
            .default_for(role)
            .cloned()
            .ok_or_else(|| Error::InvalidManifest(format!("no default {role} model")))
    }

    /// Where `entry` lives once downloaded.
    pub fn path_for(&self, entry: &ModelEntry) -> Result<PathBuf, Error> {
        let (owner, name) = entry.repo_parts()?;
        Ok(self
            .dir
            .join(owner)
            .join(name)
            .join(&entry.revision)
            .join(&entry.filename))
    }

    /// The local path if the file is present with the expected size. The
    /// SHA-256 is checked when the file is downloaded.
    pub fn installed_path(&self, entry: &ModelEntry) -> Result<Option<PathBuf>, Error> {
        let path = self.path_for(entry)?;
        Ok(match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() == entry.size => Some(path),
            _ => None,
        })
    }

    /// Like [`installed_path`](Self::installed_path), but an error telling
    /// the user how to download the model if it is missing.
    pub fn require(&self, entry: &ModelEntry) -> Result<PathBuf, Error> {
        self.installed_path(entry)?
            .ok_or_else(|| Error::NotInstalled {
                id: entry.id.clone(),
            })
    }

    pub(crate) fn partial_dir(&self, entry: &ModelEntry) -> PathBuf {
        let safe: String = entry
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.dir.join(".partial").join(safe)
    }

    fn custom_path(&self) -> PathBuf {
        self.dir.join("custom.json")
    }

    pub fn custom_entries(&self) -> Result<Vec<ModelEntry>, Error> {
        let path = self.custom_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(read_json(&path)?)
    }

    /// Records a custom model, replacing any entry with the same id.
    pub fn add_custom(&self, entry: ModelEntry) -> Result<(), Error> {
        let mut custom = self.custom_entries()?;
        custom.retain(|m| m.id != entry.id);
        custom.push(entry);
        write_json_atomic(&self.custom_path(), &custom)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, Format};

    fn custom_entry() -> ModelEntry {
        ModelEntry {
            id: "someone/asr-ft:model.q8_0.gguf".into(),
            name: "model.q8_0.gguf".into(),
            role: Role::Asr,
            engine: Engine::NemoSpeech,
            format: Format::Gguf,
            repo: "someone/asr-ft".into(),
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
            filename: "model.q8_0.gguf".into(),
            size: 4,
            sha256: "0".repeat(64),
            license: "cc-by-4.0".into(),
            license_url: "https://huggingface.co/someone/asr-ft".into(),
            default: false,
        }
    }

    #[test]
    fn installed_requires_the_expected_size() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path(), Manifest::bundled().unwrap());
        let entry = custom_entry();
        let path = store.path_for(&entry).unwrap();
        assert!(path.ends_with(Path::new(
            "someone/asr-ft/0123456789abcdef0123456789abcdef01234567/model.q8_0.gguf"
        )));

        assert_eq!(store.installed_path(&entry).unwrap(), None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(store.installed_path(&entry).unwrap(), None);
        std::fs::write(&path, b"abcd").unwrap();
        assert_eq!(store.installed_path(&entry).unwrap(), Some(path));
    }

    #[test]
    fn custom_entries_persist_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path(), Manifest::bundled().unwrap());

        store.add_custom(custom_entry()).unwrap();
        let mut updated = custom_entry();
        updated.size = 8;
        store.add_custom(updated).unwrap();

        let custom = store.custom_entries().unwrap();
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].size, 8);
        assert_eq!(
            store.find("someone/asr-ft:model.q8_0.gguf").unwrap().size,
            8
        );
        assert!(store.find("nemotron-3.5-asr").is_ok());
        assert!(matches!(store.find("nope"), Err(Error::UnknownModel(_))));
    }
}
