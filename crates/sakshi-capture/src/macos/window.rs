//! Where a window is, from the window server's list (on screen and in
//! view, behind other windows, or out of view), and bringing its app to the
//! front. The list needs Screen Recording access, which recording has.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowAlpha, kCGWindowBounds,
    kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, ns_string};

use crate::{CaptureError, Rect, WindowView, fully_covered};

/// Windows on these levels can hide a call window: ordinary windows,
/// floating panels, and the like. The Dock, the menu bar, and system
/// overlays sit higher and are mostly transparent.
const COVERING_LAYERS: std::ops::Range<i64> = 0..20;

type Entry = NSDictionary<NSString, AnyObject>;

/// One window in the window server's list.
struct Info {
    number: u32,
    pid: Option<i32>,
    layer: i64,
    alpha: f64,
    on_screen: bool,
    bounds: Option<Rect>,
}

fn list(option: CGWindowListOption, window: u32) -> Vec<Info> {
    let Some(array) = CGWindowListCopyWindowInfo(option, window) else {
        return Vec::new();
    };
    // SAFETY: the window list is documented as an array of dictionaries
    // keyed by strings.
    let array: &NSArray<Entry> = unsafe { array.cast_unchecked::<Entry>() }.as_ref();
    array.iter().map(|entry| info(&entry)).collect()
}

fn number(entry: &Entry, key: &NSString) -> Option<Retained<NSNumber>> {
    entry.objectForKey(key)?.downcast::<NSNumber>().ok()
}

fn info(entry: &Entry) -> Info {
    // SAFETY: CoreGraphics' key constants are immutable statics.
    let (number_key, pid_key, layer_key, alpha_key, on_screen_key, bounds_key): (
        &NSString,
        &NSString,
        &NSString,
        &NSString,
        &NSString,
        &NSString,
    ) = unsafe {
        (
            kCGWindowNumber.as_ref(),
            kCGWindowOwnerPID.as_ref(),
            kCGWindowLayer.as_ref(),
            kCGWindowAlpha.as_ref(),
            kCGWindowIsOnscreen.as_ref(),
            kCGWindowBounds.as_ref(),
        )
    };
    Info {
        number: number(entry, number_key).map_or(0, |n| n.unsignedIntValue()),
        pid: number(entry, pid_key).map(|n| n.intValue()),
        layer: number(entry, layer_key).map_or(0, |n| n.longLongValue()),
        alpha: number(entry, alpha_key).map_or(1.0, |n| n.doubleValue()),
        // Absent for windows that aren't on screen.
        on_screen: number(entry, on_screen_key).is_some_and(|n| n.boolValue()),
        bounds: entry
            .objectForKey(bounds_key)
            .and_then(|b| b.downcast::<NSDictionary>().ok())
            // SAFETY: a rectangle's dictionary is keyed by strings.
            .and_then(|b| rect(&*unsafe { Retained::cast_unchecked::<Entry>(b) })),
    }
}

/// A rectangle from its dictionary form, in points with the origin at the
/// top left of the main display.
fn rect(dict: &Entry) -> Option<Rect> {
    let get = |k: &NSString| number(dict, k).map(|n| n.doubleValue());
    Some(Rect {
        x: get(ns_string!("X"))?,
        y: get(ns_string!("Y"))?,
        width: get(ns_string!("Width"))?,
        height: get(ns_string!("Height"))?,
    })
}

fn find(window: u32) -> Option<Info> {
    list(CGWindowListOption::OptionIncludingWindow, window)
        .into_iter()
        .find(|w| w.number == window)
}

/// How much of `window` can be seen. A window that no longer exists counts
/// as hidden; capture reports it closing separately.
pub(super) fn view(window: u32) -> WindowView {
    let Some(me) = find(window) else {
        return WindowView::Hidden;
    };
    if !me.on_screen {
        return WindowView::Hidden;
    }
    let Some(bounds) = me.bounds else {
        return WindowView::Visible;
    };
    let above: Vec<Rect> = list(CGWindowListOption::OptionOnScreenAboveWindow, window)
        .into_iter()
        .filter(|w| {
            w.number != window && w.on_screen && w.alpha > 0.0 && COVERING_LAYERS.contains(&w.layer)
        })
        .filter_map(|w| w.bounds)
        .collect();
    if fully_covered(bounds, &above) {
        WindowView::Covered
    } else {
        WindowView::Visible
    }
}

/// Brings the app that owns `window` to the front with all its windows.
/// macOS switches to the desktop the window is on. It only allows this
/// while Sakshi is the active app, e.g. right after a click in it.
pub(super) fn bring_forward(window: u32) -> Result<(), CaptureError> {
    let not_found = || CaptureError::SourceNotFound(format!("window {window}"));
    let pid = find(window).and_then(|w| w.pid).ok_or_else(not_found)?;
    let app =
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid).ok_or_else(not_found)?;
    let me = NSRunningApplication::currentApplication();
    if app.activateFromApplication_options(&me, NSApplicationActivationOptions::ActivateAllWindows)
    {
        Ok(())
    } else {
        Err(CaptureError::Platform(
            "macOS didn't let Sakshi bring that window forward".into(),
        ))
    }
}
