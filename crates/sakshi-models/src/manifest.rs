use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Error;

/// What a model does in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Asr,
    Diarization,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Asr => "asr",
            Self::Diarization => "diarization",
        })
    }
}

/// The runtime that executes a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    NemoSpeech,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Gguf,
}

/// One downloadable model file, pinned to a repository revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub engine: Engine,
    pub format: Format,
    /// Hugging Face repository, `owner/name`.
    pub repo: String,
    /// Full commit hash the download is pinned to.
    pub revision: String,
    pub filename: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub license: String,
    pub license_url: String,
    /// Used when no model is chosen explicitly for this role.
    #[serde(default)]
    pub default: bool,
}

impl ModelEntry {
    /// `(owner, name)` of the repository.
    pub fn repo_parts(&self) -> Result<(&str, &str), Error> {
        split_repo(&self.repo)
    }

    pub fn repo_url(&self) -> String {
        format!("https://huggingface.co/{}", self.repo)
    }
}

pub(crate) fn split_repo(repo: &str) -> Result<(&str, &str), Error> {
    // Repo ids become path components under the models folder, so `..` and
    // other dot-leading names are rejected along with separators.
    let valid_part = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    match repo.split_once('/') {
        Some((owner, name)) if valid_part(owner) && valid_part(name) => Ok((owner, name)),
        _ => Err(Error::InvalidRepoId(repo.to_owned())),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub models: Vec<ModelEntry>,
}

impl Manifest {
    /// The manifest compiled into this build.
    pub fn bundled() -> Result<Self, Error> {
        let manifest: Self = serde_json::from_str(include_str!("../manifest.json"))
            .map_err(|e| Error::InvalidManifest(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn find(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn default_for(&self, role: Role) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.role == role && m.default)
    }

    fn validate(&self) -> Result<(), Error> {
        let fail = |msg: String| Err(Error::InvalidManifest(msg));
        for (i, m) in self.models.iter().enumerate() {
            if self.models[..i].iter().any(|other| other.id == m.id) {
                return fail(format!("duplicate id {}", m.id));
            }
            split_repo(&m.repo)?;
            if !is_hex(&m.revision, 40) {
                return fail(format!("{}: revision must be a full commit hash", m.id));
            }
            if !is_hex(&m.sha256, 64) {
                return fail(format!("{}: sha256 must be 64 lowercase hex digits", m.id));
            }
        }
        for role in [Role::Asr, Role::Diarization] {
            let defaults = self
                .models
                .iter()
                .filter(|m| m.role == role && m.default)
                .count();
            if defaults != 1 {
                return fail(format!(
                    "expected one default {role} model, found {defaults}"
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_manifest_is_valid() {
        let manifest = Manifest::bundled().unwrap();

        let asr = manifest.default_for(Role::Asr).unwrap();
        assert_eq!(asr.repo, "nvidia/nemotron-3.5-asr-streaming-0.6b");
        assert_eq!(asr.filename, "nemotron-3.5-asr-streaming-0.6b.q8_0.gguf");
        let diar = manifest.default_for(Role::Diarization).unwrap();
        assert_eq!(diar.repo, "nvidia/Nemotron-3-Diarization");
        // Only official NVIDIA repositories are bundled.
        assert!(
            manifest
                .models
                .iter()
                .all(|m| m.repo.starts_with("nvidia/"))
        );
    }

    #[test]
    fn repo_ids_are_checked() {
        assert_eq!(
            split_repo("nvidia/model-1.0").unwrap(),
            ("nvidia", "model-1.0")
        );
        for bad in [
            "nvidia",
            "/model",
            "nvidia/",
            "a/b/c",
            "nv idia/x",
            "../etc",
        ] {
            assert!(split_repo(bad).is_err(), "{bad:?} should fail");
        }
    }
}
