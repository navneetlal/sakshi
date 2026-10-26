//! Screen Recording and Microphone access (TCC).
//!
//! For a command-line tool, macOS attributes access to the app that launched
//! it, such as Terminal or iTerm, so that is the app to enable in System
//! Settings.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationRecordPermission};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};

use crate::PermissionState;

/// Screen Recording access. macOS doesn't distinguish "never asked" from
/// "denied" here, so both report `Denied`.
pub(super) fn screen() -> PermissionState {
    if CGPreflightScreenCaptureAccess() {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

pub(super) fn request_screen() -> bool {
    CGRequestScreenCaptureAccess()
}

pub(super) fn microphone() -> PermissionState {
    let permission = unsafe { AVAudioApplication::sharedInstance().recordPermission() };
    match permission {
        AVAudioApplicationRecordPermission::Granted => PermissionState::Granted,
        AVAudioApplicationRecordPermission::Denied => PermissionState::Denied,
        _ => PermissionState::NotDetermined,
    }
}

/// Shows the system prompt if the user hasn't answered yet and waits (up to
/// two minutes) for the answer.
pub(super) fn request_microphone() -> PermissionState {
    if microphone() != PermissionState::NotDetermined {
        return microphone();
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |granted: Bool| {
        let _ = tx.try_send(granted.as_bool());
    });
    unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
    match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(true) => PermissionState::Granted,
        Ok(false) => PermissionState::Denied,
        Err(_) => microphone(),
    }
}
