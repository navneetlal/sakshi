//! The model manager: what's downloaded, downloads with progress, and
//! adding a GGUF model from any Hugging Face repository. Every network
//! request here starts from a button the user pressed.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use sakshi_core::DataDir;
use sakshi_models::{Hub, ModelEntry, ModelStore};
use serde::Serialize;

use crate::events::{AppEvent, Notify};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadView {
    pub model_id: String,
    pub done: u64,
    pub total: u64,
    #[serde(flatten)]
    pub state: DownloadState,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadState {
    Running,
    Done,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    #[serde(flatten)]
    pub entry: ModelEntry,
    pub repo_url: String,
    pub installed: bool,
    /// Added by the user rather than shipped with Sakshi.
    pub custom: bool,
    pub download: Option<DownloadView>,
}

/// A custom model that passed validation, waiting for the user to accept
/// its license.
#[derive(Debug, Clone, Serialize)]
pub struct InspectedModel {
    #[serde(flatten)]
    pub model: ModelView,
    /// `general.architecture` from the file's header.
    pub architecture: String,
}

pub struct Models {
    data: DataDir,
    notify: Arc<dyn Notify>,
    downloads: Arc<Mutex<HashMap<String, DownloadView>>>,
    inspected: Mutex<HashMap<String, ModelEntry>>,
}

impl Models {
    pub fn new(data: DataDir, notify: Arc<dyn Notify>) -> Self {
        Self {
            data,
            notify,
            downloads: Arc::default(),
            inspected: Mutex::default(),
        }
    }

    pub fn list(&self) -> anyhow::Result<Vec<ModelView>> {
        let store = ModelStore::open(&self.data)?;
        let custom: HashSet<String> = store.custom_entries()?.into_iter().map(|e| e.id).collect();
        store
            .entries()?
            .into_iter()
            .map(|entry| {
                let custom = custom.contains(&entry.id);
                self.view(&store, entry, custom)
            })
            .collect()
    }

    fn view(
        &self,
        store: &ModelStore,
        entry: ModelEntry,
        custom: bool,
    ) -> anyhow::Result<ModelView> {
        Ok(ModelView {
            repo_url: entry.repo_url(),
            installed: store.installed_path(&entry)?.is_some(),
            custom,
            download: self
                .downloads
                .lock()
                .expect("downloads")
                .get(&entry.id)
                .cloned(),
            entry,
        })
    }

    /// Starts downloading a known model in the background. The user must
    /// have accepted its license.
    pub fn download(&self, id: &str, license_accepted: bool) -> anyhow::Result<()> {
        let store = ModelStore::open(&self.data)?;
        let entry = store.find(id)?;
        if !license_accepted {
            bail!(
                "accept the {} license to download {}",
                entry.license,
                entry.name
            );
        }
        if store.installed_path(&entry)?.is_some() {
            return Ok(());
        }
        let hub = Hub::new()?;
        {
            let mut downloads = self.downloads.lock().expect("downloads");
            if downloads
                .get(id)
                .is_some_and(|d| d.state == DownloadState::Running)
            {
                return Ok(());
            }
            downloads.insert(
                entry.id.clone(),
                DownloadView {
                    model_id: entry.id.clone(),
                    done: 0,
                    total: entry.size,
                    state: DownloadState::Running,
                },
            );
        }
        self.publish(&entry.id);

        let downloads = Arc::clone(&self.downloads);
        let notify = Arc::clone(&self.notify);
        std::thread::Builder::new()
            .name("sakshi-download".into())
            .spawn(move || {
                let id = entry.id.clone();
                let last_percent = Mutex::new(u64::MAX);
                let progress_downloads = Arc::clone(&downloads);
                let progress_notify = Arc::clone(&notify);
                let progress_id = id.clone();
                let progress: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(move |done, total| {
                    let percent = (done * 100).checked_div(total).unwrap_or(0);
                    let view = {
                        let mut downloads = progress_downloads.lock().expect("downloads");
                        let Some(view) = downloads.get_mut(&progress_id) else {
                            return;
                        };
                        view.done = done;
                        view.total = total;
                        view.clone()
                    };
                    // One event per percent is plenty for a progress bar.
                    let mut last = last_percent.lock().expect("progress");
                    if *last != percent {
                        *last = percent;
                        progress_notify.notify(AppEvent::Download { download: view });
                    }
                });
                let result = hub.download(&store, &entry, true, Some(progress));
                let view = {
                    let mut downloads = downloads.lock().expect("downloads");
                    let Some(view) = downloads.get_mut(&id) else {
                        return;
                    };
                    view.state = match result {
                        Ok(_) => DownloadState::Done,
                        Err(e) => {
                            tracing::warn!(model = %id, "download failed: {e}");
                            DownloadState::Failed {
                                error: e.to_string(),
                            }
                        }
                    };
                    view.clone()
                };
                notify.notify(AppEvent::Download { download: view });
            })
            .context("starting the download")?;
        Ok(())
    }

    /// Checks a GGUF file in a Hugging Face repository without downloading
    /// it: it must be a model the nemo-speech runtime can run.
    pub fn inspect(
        &self,
        repo: &str,
        filename: &str,
        revision: Option<&str>,
    ) -> anyhow::Result<InspectedModel> {
        let custom = Hub::new()?.inspect_custom(repo.trim(), filename.trim(), revision)?;
        let store = ModelStore::open(&self.data)?;
        let model = self.view(&store, custom.entry.clone(), true)?;
        self.inspected
            .lock()
            .expect("inspected models")
            .insert(custom.entry.id.clone(), custom.entry);
        Ok(InspectedModel {
            model,
            architecture: custom.architecture,
        })
    }

    /// Adds a model [`Models::inspect`] approved and starts downloading it.
    pub fn add(&self, id: &str, license_accepted: bool) -> anyhow::Result<()> {
        if !license_accepted {
            bail!("accept the model's license to add it");
        }
        let entry = self
            .inspected
            .lock()
            .expect("inspected models")
            .remove(id)
            .context("check the model again before adding it")?;
        ModelStore::open(&self.data)?.add_custom(entry)?;
        self.download(id, true)
    }

    fn publish(&self, id: &str) {
        let view = self.downloads.lock().expect("downloads").get(id).cloned();
        if let Some(download) = view {
            self.notify.notify(AppEvent::Download { download });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::testing::Recorded;

    fn models(root: &std::path::Path) -> (Models, Arc<Recorded>) {
        let notify = Arc::new(Recorded::default());
        (Models::new(DataDir::new(root), notify.clone()), notify)
    }

    #[test]
    fn lists_the_bundled_models_as_not_downloaded() {
        let root = tempfile::tempdir().unwrap();
        let (models, _) = models(root.path());

        let list = models.list().unwrap();

        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .all(|m| !m.installed && !m.custom && m.download.is_none())
        );
        assert!(list.iter().all(|m| m.entry.default));
    }

    #[test]
    fn downloads_need_the_license_accepted_first() {
        let root = tempfile::tempdir().unwrap();
        let (models, notify) = models(root.path());
        let id = models.list().unwrap()[0].entry.id.clone();

        let err = models.download(&id, false).unwrap_err();

        assert!(err.to_string().contains("license"), "{err}");
        assert!(notify.take().is_empty());
        assert!(models.download("no-such-model", true).is_err());
    }

    #[test]
    fn only_inspected_models_can_be_added() {
        let root = tempfile::tempdir().unwrap();
        let (models, _) = models(root.path());
        let err = models.add("owner/repo:model.gguf", true).unwrap_err();
        assert!(err.to_string().contains("check the model again"), "{err}");
    }
}
