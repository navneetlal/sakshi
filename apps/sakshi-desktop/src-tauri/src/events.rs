//! What the backend tells the UI without being asked. Everything goes out
//! as one Tauri event, [`EVENT`], whose payload's `kind` says what changed.

use serde::Serialize;
use tauri::Emitter;

use crate::jobs::JobView;
use crate::models::DownloadView;
use crate::recorder::RecorderStatus;

pub const EVENT: &str = "sakshi";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppEvent {
    Recorder {
        status: RecorderStatus,
    },
    Job {
        job: JobView,
    },
    Download {
        download: DownloadView,
    },
    /// A meeting was added to the index or changed.
    MeetingsChanged,
}

/// Where events go: the app's windows, or a list in tests.
pub trait Notify: Send + Sync {
    fn notify(&self, event: AppEvent);
}

impl Notify for tauri::AppHandle {
    fn notify(&self, event: AppEvent) {
        if let Err(e) = self.emit(EVENT, &event) {
            tracing::warn!("couldn't send an event to the UI: {e}");
        }
    }
}

#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use super::*;

    /// Keeps every event for a test to look at.
    #[derive(Default)]
    pub struct Recorded(pub Mutex<Vec<AppEvent>>);

    impl Recorded {
        pub fn take(&self) -> Vec<AppEvent> {
            std::mem::take(&mut self.0.lock().unwrap())
        }
    }

    impl Notify for Recorded {
        fn notify(&self, event: AppEvent) {
            self.0.lock().unwrap().push(event);
        }
    }
}
