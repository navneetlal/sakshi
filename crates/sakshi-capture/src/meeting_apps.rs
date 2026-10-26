//! Recognizing windows that are probably a call.

const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.google.Chrome.beta",
    "com.apple.Safari",
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
];

/// The meeting app a window belongs to, if it looks like one: the Zoom and
/// Teams desktop apps, or a browser tab whose title suggests Google Meet,
/// Teams, or Zoom on the web.
pub fn meeting_label(bundle_id: Option<&str>, title: &str) -> Option<&'static str> {
    let title = title.to_lowercase();
    match bundle_id? {
        "us.zoom.xos" => Some("Zoom"),
        "com.microsoft.teams2" | "com.microsoft.teams" => Some("Microsoft Teams"),
        "com.cisco.webexmeetingsapp" | "Cisco-Systems.Spark" => Some("Webex"),
        b if BROWSERS.contains(&b) => {
            if title.contains("google meet")
                || title.contains("meet.google.com")
                || title.starts_with("meet - ")
                || title.starts_with("meet – ")
            {
                Some("Google Meet")
            } else if title.contains("microsoft teams") {
                Some("Microsoft Teams")
            } else if title.contains("zoom meeting") || title.contains("zoom.us") {
                Some("Zoom")
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_desktop_apps() {
        assert_eq!(
            meeting_label(Some("us.zoom.xos"), "Zoom Meeting"),
            Some("Zoom")
        );
        assert_eq!(
            meeting_label(
                Some("com.microsoft.teams2"),
                "Weekly sync | Microsoft Teams"
            ),
            Some("Microsoft Teams")
        );
        assert_eq!(
            meeting_label(Some("com.apple.TextEdit"), "Zoom notes"),
            None
        );
        assert_eq!(meeting_label(None, "Google Meet"), None);
    }

    #[test]
    fn recognizes_meetings_in_browsers() {
        assert_eq!(
            meeting_label(
                Some("com.google.Chrome"),
                "Meet - abc-defg-hij - Google Chrome"
            ),
            Some("Google Meet")
        );
        assert_eq!(
            meeting_label(Some("com.apple.Safari"), "meet.google.com/abc-defg-hij"),
            Some("Google Meet")
        );
        assert_eq!(
            meeting_label(
                Some("company.thebrowser.Browser"),
                "Standup | Microsoft Teams"
            ),
            Some("Microsoft Teams")
        );
        assert_eq!(
            meeting_label(Some("com.google.Chrome"), "Inbox - Gmail"),
            None
        );
        assert_eq!(
            meeting_label(Some("com.google.Chrome"), "How to meet deadlines"),
            None
        );
    }
}
