//! Listing windows and displays, and building the filter for one.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{SCContentFilter, SCShareableContent, SCShareableContentInfo};

use crate::{CaptureError, Source, SourceId, meeting_label};

/// Windows smaller than this (in points) are toolbars, popovers, and the like.
const MIN_WINDOW_SIDE: f64 = 120.0;

/// Shareable content, made sendable for the trip back from the completion
/// handler's queue.
pub(super) struct Content(Retained<SCShareableContent>);

// SAFETY: SCShareableContent is an immutable snapshot; Apple documents the
// ScreenCaptureKit content types as safe to use from any thread.
unsafe impl Send for Content {}

fn describe(error: *mut NSError) -> String {
    match unsafe { error.as_ref() } {
        Some(e) => e.localizedDescription().to_string(),
        None => "unknown error".into(),
    }
}

/// Opens this process's connection to the window server. AppKit apps get
/// one at launch; a command-line tool doesn't, and parts of
/// ScreenCaptureKit (such as a filter's pixel scale) then abort with
/// `CGS_REQUIRE_INIT`. Asking CoreGraphics for the main display opens it.
pub(super) fn ensure_window_server() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = objc2_core_graphics::CGMainDisplayID();
    });
}

/// On-screen windows and displays. Fails with a permission error when
/// Screen Recording access is missing.
pub(super) fn shareable_content() -> Result<Content, CaptureError> {
    ensure_window_server();
    let (tx, rx) = mpsc::sync_channel::<Result<Content, String>>(1);
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(Content(content)),
                None => Err(describe(error)),
            };
            let _ = tx.try_send(result);
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &block,
        );
    }
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(content)) => Ok(content),
        Ok(Err(message)) => {
            if super::permissions::screen() != crate::PermissionState::Granted {
                Err(CaptureError::PermissionDenied(format!(
                    "Screen Recording access is off ({message})"
                )))
            } else {
                Err(CaptureError::Platform(format!(
                    "listing windows failed: {message}"
                )))
            }
        }
        Err(_) => Err(CaptureError::Platform(
            "ScreenCaptureKit did not answer within 15 s".into(),
        )),
    }
}

pub(super) fn sources() -> Result<Vec<Source>, CaptureError> {
    Ok(list(&shareable_content()?))
}

fn list(content: &Content) -> Vec<Source> {
    let mut out = Vec::new();
    let own_pid = std::process::id();
    for window in unsafe { content.0.windows() }.iter() {
        let frame = unsafe { window.frame() };
        let (w, h) = (frame.size.width, frame.size.height);
        if unsafe { window.windowLayer() } != 0 || w < MIN_WINDOW_SIDE || h < MIN_WINDOW_SIDE {
            continue;
        }
        let app = unsafe { window.owningApplication() };
        if app
            .as_ref()
            .is_some_and(|a| u32::try_from(unsafe { a.processID() }).ok() == Some(own_pid))
        {
            continue;
        }
        let title = unsafe { window.title() }
            .map(|t| t.to_string())
            .unwrap_or_default();
        let bundle_id = app
            .as_ref()
            .map(|a| unsafe { a.bundleIdentifier() }.to_string());
        let app_name = app
            .as_ref()
            .map(|a| unsafe { a.applicationName() }.to_string());
        if title.is_empty() && app_name.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        out.push(Source {
            id: SourceId::Window(unsafe { window.windowID() }),
            meeting: meeting_label(bundle_id.as_deref(), &title).map(str::to_owned),
            title,
            app_name,
            bundle_id,
            width: w as u32,
            height: h as u32,
        });
    }
    for display in unsafe { content.0.displays() }.iter() {
        let id = unsafe { display.displayID() };
        let (w, h) = unsafe { (display.width(), display.height()) };
        out.push(Source {
            id: SourceId::Display(id),
            title: format!("Display {id}"),
            app_name: None,
            bundle_id: None,
            width: u32::try_from(w).unwrap_or(0),
            height: u32::try_from(h).unwrap_or(0),
            meeting: None,
        });
    }
    out
}

/// What to capture: the filter, the source's description, and its
/// pixels-per-point scale.
pub(super) struct Target {
    pub filter: Retained<SCContentFilter>,
    pub source: Source,
    pub scale: u32,
}

pub(super) fn target(content: &Content, id: SourceId) -> Result<Target, CaptureError> {
    let source = list(content)
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| CaptureError::SourceNotFound(id.to_string()))?;
    let filter = match id {
        SourceId::Window(window_id) => {
            let window = unsafe { content.0.windows() }
                .iter()
                .find(|w| unsafe { w.windowID() } == window_id)
                .ok_or_else(|| CaptureError::SourceNotFound(id.to_string()))?;
            unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
            }
        }
        SourceId::Display(display_id) => {
            let display = unsafe { content.0.displays() }
                .iter()
                .find(|d| unsafe { d.displayID() } == display_id)
                .ok_or_else(|| CaptureError::SourceNotFound(id.to_string()))?;
            unsafe {
                SCContentFilter::initWithDisplay_excludingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &NSArray::new(),
                )
            }
        }
    };
    let info = unsafe { SCShareableContent::infoForFilter(&filter) };
    let scale = scale_of(&info);
    Ok(Target {
        filter,
        source,
        scale,
    })
}

fn scale_of(info: &SCShareableContentInfo) -> u32 {
    let scale = unsafe { info.pointPixelScale() };
    if scale.is_finite() && scale >= 1.0 {
        scale.round() as u32
    } else {
        1
    }
}
