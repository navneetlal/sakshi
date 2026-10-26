use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use hf_hub::HFClientSync;
use hf_hub::progress::{DownloadEvent, Progress, ProgressEvent, ProgressHandler};
use sha2::{Digest, Sha256};

use crate::gguf::{GgufError, GgufMetadata};
use crate::manifest::{is_hex, split_repo};
use crate::{Engine, Error, Format, ModelEntry, ModelStore, Role};

/// Called with `(bytes_done, bytes_total)` while a download runs.
pub type DownloadProgress = dyn Fn(u64, u64) + Send + Sync;

/// Header prefixes tried, in order, when probing a remote GGUF file.
const PROBE_SIZES: [u64; 4] = [1 << 20, 4 << 20, 16 << 20, 64 << 20];

/// Hugging Face access. Creating a `Hub` makes no requests; each method is
/// one user-initiated operation.
pub struct Hub {
    client: HFClientSync,
    http: reqwest::blocking::Client,
    endpoint: String,
    token: Option<String>,
}

impl std::fmt::Debug for Hub {
    // Hand-written so the token never reaches logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("endpoint", &self.endpoint)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// A custom model that passed validation and is ready to download.
#[derive(Debug, Clone)]
pub struct CustomModel {
    pub entry: ModelEntry,
    /// `general.architecture` from the file's GGUF header.
    pub architecture: String,
}

impl Hub {
    /// Uses the standard Hugging Face environment (`HF_TOKEN`,
    /// `HF_ENDPOINT`) so gated repositories work with the user's token.
    pub fn new() -> Result<Self, Error> {
        let env = |key| std::env::var(key).ok().filter(|v| !v.is_empty());
        Ok(Self {
            client: HFClientSync::new()?,
            http: reqwest::blocking::Client::builder()
                .user_agent(concat!("sakshi/", env!("CARGO_PKG_VERSION")))
                .build()?,
            endpoint: env("HF_ENDPOINT")
                .unwrap_or_else(|| "https://huggingface.co".into())
                .trim_end_matches('/')
                .to_owned(),
            token: env("HF_TOKEN"),
        })
    }

    /// Downloads `entry` into `store` unless it is already there, then checks
    /// its size and SHA-256 before moving it into place.
    pub fn download(
        &self,
        store: &ModelStore,
        entry: &ModelEntry,
        license_accepted: bool,
        progress: Option<Arc<DownloadProgress>>,
    ) -> Result<PathBuf, Error> {
        if let Some(path) = store.installed_path(entry)? {
            return Ok(path);
        }
        if !license_accepted {
            return Err(Error::LicenseNotAccepted {
                id: entry.id.clone(),
                license: entry.license.clone(),
            });
        }
        let (owner, name) = entry.repo_parts()?;
        let staging = store.partial_dir(entry);
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| Error::io(&staging, e))?;
        }
        std::fs::create_dir_all(&staging).map_err(|e| Error::io(&staging, e))?;

        tracing::info!(model = %entry.id, repo = %entry.repo, revision = %entry.revision, "downloading");
        let downloaded = self
            .client
            .model(owner, name)
            .download_file()
            .filename(&entry.filename)
            .revision(&entry.revision)
            .local_dir(staging.clone())
            .maybe_progress(progress.map(|p| Progress::new(Reporter(p))))
            .send()?;

        verify(&downloaded, entry.size, &entry.sha256)?;
        let dest = store.path_for(entry)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        std::fs::rename(&downloaded, &dest).map_err(|e| Error::io(&dest, e))?;
        std::fs::remove_dir_all(&staging).map_err(|e| Error::io(&staging, e))?;
        Ok(dest)
    }

    /// Validates a user-supplied model before anything is downloaded: the
    /// file must be GGUF, and its header (fetched with range requests) must
    /// declare an architecture the nemo-speech runtime runs. The returned
    /// entry is pinned to the commit `revision` resolves to.
    pub fn inspect_custom(
        &self,
        repo: &str,
        filename: &str,
        revision: Option<&str>,
    ) -> Result<CustomModel, Error> {
        check_custom_filename(filename)?;
        let (owner, name) = split_repo(repo)?;
        let model = self.client.model(owner, name);

        let info = model.info().maybe_revision(revision).send()?;
        let commit = info
            .sha
            .filter(|sha| is_hex(sha, 40))
            .ok_or_else(|| Error::FileNotFound {
                repo: repo.to_owned(),
                filename: filename.to_owned(),
                revision: revision.unwrap_or("main").to_owned(),
            })?;
        let (size, sha256) = self.lfs_file_info(repo, &commit, filename)?;
        let header = self.probe_header(&model, filename, &commit, size)?;
        let architecture = header.architecture().unwrap_or_default().to_owned();
        let role = match architecture.as_str() {
            "asr" => Role::Asr,
            "sortformer" => Role::Diarization,
            other => {
                return Err(Error::UnsupportedModel {
                    filename: filename.to_owned(),
                    reason: format!("its architecture is {other:?}"),
                });
            }
        };

        Ok(CustomModel {
            entry: ModelEntry {
                id: format!("{repo}:{filename}"),
                name: filename.to_owned(),
                role,
                engine: Engine::NemoSpeech,
                format: Format::Gguf,
                repo: repo.to_owned(),
                revision: commit,
                filename: filename.to_owned(),
                size,
                sha256,
                license: license_name(info.card_data.as_ref()),
                license_url: format!("https://huggingface.co/{repo}"),
                default: false,
            },
            architecture,
        })
    }

    /// Size and SHA-256 of a file from the Hub's `paths-info` API.
    ///
    /// hf-hub 1.0.0 can't supply these: its tree types read the LFS hash
    /// from a `sha256` field while the API sends `oid`, and its HEAD-based
    /// metadata call fails on Xet-backed files after the CDN redirect.
    fn lfs_file_info(
        &self,
        repo: &str,
        revision: &str,
        filename: &str,
    ) -> Result<(u64, String), Error> {
        #[derive(serde::Deserialize)]
        struct Entry {
            path: String,
            size: u64,
            lfs: Option<Lfs>,
        }
        #[derive(serde::Deserialize)]
        struct Lfs {
            oid: String,
        }

        let url = format!("{}/api/models/{repo}/paths-info/{revision}", self.endpoint);
        let mut request = self
            .http
            .post(url)
            .json(&serde_json::json!({ "paths": [filename] }));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let entries: Vec<Entry> = request.send()?.error_for_status()?.json()?;
        let entry = entries
            .into_iter()
            .find(|e| e.path == filename)
            .ok_or_else(|| Error::FileNotFound {
                repo: repo.to_owned(),
                filename: filename.to_owned(),
                revision: revision.to_owned(),
            })?;
        let sha256 = entry
            .lfs
            .map(|l| l.oid.to_ascii_lowercase())
            .filter(|s| is_hex(s, 64))
            .ok_or_else(|| Error::UnsupportedModel {
                filename: filename.to_owned(),
                reason: "the Hub reports no SHA-256 for this file, so it cannot be verified".into(),
            })?;
        Ok((entry.size, sha256))
    }

    fn probe_header(
        &self,
        model: &hf_hub::HFRepositorySync<hf_hub::repository::RepoTypeModel>,
        filename: &str,
        revision: &str,
        size: u64,
    ) -> Result<GgufMetadata, Error> {
        let gguf_err = |source| match source {
            GgufError::NotGguf => Error::UnsupportedModel {
                filename: filename.to_owned(),
                reason: "the file is not GGUF".into(),
            },
            source => Error::Gguf {
                filename: filename.to_owned(),
                source,
            },
        };
        for want in PROBE_SIZES {
            let end = want.min(size);
            let bytes = model
                .download_file_to_bytes()
                .filename(filename)
                .revision(revision)
                .range(0..end)
                .send()?;
            match GgufMetadata::parse(&bytes) {
                Err(GgufError::Truncated(_)) if end < size => continue,
                other => return other.map_err(gguf_err),
            }
        }
        Err(gguf_err(GgufError::Malformed(format!(
            "metadata header is larger than {} MiB",
            PROBE_SIZES[PROBE_SIZES.len() - 1] >> 20
        ))))
    }
}

/// Rejects anything but a plain relative `.gguf` path, with a reason the
/// user can act on.
fn check_custom_filename(filename: &str) -> Result<(), Error> {
    let reject = |reason: &str| {
        Err(Error::UnsupportedModel {
            filename: filename.to_owned(),
            reason: reason.to_owned(),
        })
    };
    let path = Path::new(filename);
    if filename.is_empty()
        || filename.contains('\\')
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return reject("expected a file path inside the repository");
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "gguf" => Ok(()),
        "nemo" => {
            reject("this is a NeMo checkpoint; converting checkpoints to GGUF is not supported yet")
        }
        "" => reject("the file has no extension"),
        other => reject(&format!(".{other} files are not supported")),
    }
}

/// A display string for the license declared on a model card.
fn license_name(card: Option<&serde_json::Value>) -> String {
    let field = |key| card.and_then(|c| c.get(key));
    let license = match field("license") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        _ => return "not declared; read the model card before use".into(),
    };
    match (
        license.as_str(),
        field("license_name").and_then(|v| v.as_str()),
    ) {
        ("other", Some(name)) => name.to_owned(),
        _ => license,
    }
}

fn verify(path: &Path, size: u64, sha256: &str) -> Result<(), Error> {
    let actual_size = std::fs::metadata(path)
        .map_err(|e| Error::io(path, e))?
        .len();
    if actual_size != size {
        return Err(Error::SizeMismatch {
            path: path.to_path_buf(),
            expected: size,
            actual: actual_size,
        });
    }
    let actual = sha256_file(path).map_err(|e| Error::io(path, e))?;
    if actual != sha256 {
        return Err(Error::ChecksumMismatch {
            path: path.to_path_buf(),
            expected: sha256.to_owned(),
            actual,
        });
    }
    Ok(())
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(hex::encode(hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

struct Reporter(Arc<DownloadProgress>);

impl ProgressHandler for Reporter {
    fn on_progress(&self, event: &ProgressEvent) {
        match event {
            ProgressEvent::Download(DownloadEvent::Progress { files }) => {
                for f in files {
                    (self.0)(f.bytes_completed, f.total_bytes);
                }
            }
            ProgressEvent::Download(DownloadEvent::AggregateProgress {
                bytes_completed,
                total_bytes,
                ..
            }) => (self.0)(*bytes_completed, *total_bytes),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_filenames_must_be_gguf() {
        assert!(check_custom_filename("model.q8_0.gguf").is_ok());
        assert!(check_custom_filename("sub/model.GGUF").is_ok());
        for (bad, why) in [
            ("model.nemo", "NeMo checkpoint"),
            ("ggml-base.bin", ".bin files"),
            ("model.safetensors", ".safetensors files"),
            ("README", "no extension"),
            ("../model.gguf", "inside the repository"),
            ("/abs/model.gguf", "inside the repository"),
            ("a\\b.gguf", "inside the repository"),
        ] {
            let err = check_custom_filename(bad).unwrap_err().to_string();
            assert!(err.contains(why), "{bad}: {err}");
        }
    }

    #[test]
    fn license_names_come_from_the_card() {
        let card = serde_json::json!({"license": "other", "license_name": "openmdw-1.1"});
        assert_eq!(license_name(Some(&card)), "openmdw-1.1");
        let card = serde_json::json!({"license": ["mit", "apache-2.0"]});
        assert_eq!(license_name(Some(&card)), "mit, apache-2.0");
        assert!(license_name(None).contains("model card"));
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"abc").unwrap();
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        assert!(verify(&path, 3, sha).is_ok());
        assert!(matches!(
            verify(&path, 4, sha),
            Err(Error::SizeMismatch { .. })
        ));
        assert!(matches!(
            verify(&path, 3, &"0".repeat(64)),
            Err(Error::ChecksumMismatch { .. })
        ));
    }
}
