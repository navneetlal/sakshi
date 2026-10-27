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

    /// Tells the user something that can't wait until they look at
    /// Sakshi: a system notification.
    fn alert(&self, title: &str, body: &str);
}

impl Notify for tauri::AppHandle {
    fn notify(&self, event: AppEvent) {
        if let Err(e) = self.emit(EVENT, &event) {
            tracing::warn!("couldn't send an event to the UI: {e}");
        }
    }

    fn alert(&self, title: &str, body: &str) {
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self.notification().builder().title(title).body(body).show() {
            tracing::warn!("couldn't show a notification: {e}");
        }
    }
}

#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use super::*;

    /// Keeps every event and alert for a test to look at.
    #[derive(Default)]
    pub struct Recorded {
        events: Mutex<Vec<AppEvent>>,
        alerts: Mutex<Vec<(String, String)>>,
    }

    impl Recorded {
        pub fn take(&self) -> Vec<AppEvent> {
            std::mem::take(&mut self.events.lock().unwrap())
        }

        /// (title, body) of each alert so far.
        pub fn take_alerts(&self) -> Vec<(String, String)> {
            std::mem::take(&mut self.alerts.lock().unwrap())
        }
    }

    impl Notify for Recorded {
        fn notify(&self, event: AppEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn alert(&self, title: &str, body: &str) {
            self.alerts
                .lock()
                .unwrap()
                .push((title.into(), body.into()));
        }
    }
}
