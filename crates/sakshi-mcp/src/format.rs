//! Plain-text rendering of transcripts and times.

use sakshi_core::{Segment, VideoPause, WindowView};

/// `m:ss` below an hour, `h:mm:ss` from an hour.
pub(crate) fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "video paused 0:12–0:30 (the call window was minimized or on another
/// desktop)".
pub(crate) fn pause_text(pause: &VideoPause) -> String {
    let why = match pause.window {
        WindowView::Covered => "behind other windows",
        WindowView::Hidden => "minimized or on another desktop",
        WindowView::Visible => "out of view",
    };
    format!(
        "video paused {}–{} (the call window was {why})",
        clock(pause.from_ms),
        clock(pause.to_ms)
    )
}

/// The pause `at_ms` falls in, if any.
pub(crate) fn pause_at(pauses: &[VideoPause], at_ms: u64) -> Option<&VideoPause> {
    pauses.iter().find(|p| p.contains(at_ms))
}

/// One line per segment, with a marker whenever a different key frame is on
/// screen, and where the video paused.
pub(crate) fn transcript_text(
    title: &str,
    meeting_id: &str,
    segments: &[(Segment, Option<u64>)],
    pauses: &[VideoPause],
    next_cursor: Option<&str>,
) -> String {
    let mut out = format!(
        "Transcript of \"{title}\" ({meeting_id}). Times are from the start of the recording.\n"
    );
    let mut on_screen = None;
    let mut in_pause = None;
    for (s, frame) in segments {
        let pause = pause_at(pauses, s.start_ms);
        if let Some(p) = pause
            && pause != in_pause
        {
            out.push_str(&format!(
                "-- {}: the key frame on screen may be out of date --\n",
                pause_text(p)
            ));
        }
        in_pause = pause;
        if frame.is_some() && *frame != on_screen {
            let at = frame.expect("checked");
            out.push_str(&format!(
                "-- key frame {at} on screen (from {}) --\n",
                clock(at)
            ));
            on_screen = *frame;
        }
        out.push_str(&format!(
            "[{}] {}: {}\n",
            clock(s.start_ms),
            s.speaker,
            s.text
        ));
    }
    match next_cursor {
        Some(cursor) => out.push_str(&format!(
            "(more follows: call get_transcript again with cursor \"{cursor}\")\n"
        )),
        None => out.push_str("(end of transcript)\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use sakshi_core::{Speaker, Track};

    use super::*;

    #[test]
    fn clocks() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(61_999), "1:01");
        assert_eq!(clock(3_725_000), "1:02:05");
    }

    #[test]
    fn renders_lines_and_frame_changes() {
        let seg = |id, start_ms, speaker, text: &str| Segment {
            id,
            start_ms,
            end_ms: start_ms + 500,
            track: Track::System,
            speaker,
            text: text.into(),
            words: vec![],
        };
        let text = transcript_text(
            "Standup",
            "m1",
            &[
                (seg(0, 1000, Speaker::Remote(1), "hello"), Some(0)),
                (seg(1, 2000, Speaker::You, "hi"), Some(0)),
                (
                    seg(2, 21_000, Speaker::Remote(2), "next slide"),
                    Some(20_000),
                ),
            ],
            &[],
            Some("2"),
        );
        assert_eq!(
            text,
            "Transcript of \"Standup\" (m1). Times are from the start of the recording.\n\
             -- key frame 0 on screen (from 0:00) --\n\
             [0:01] Speaker 1: hello\n\
             [0:02] You: hi\n\
             -- key frame 20000 on screen (from 0:20) --\n\
             [0:21] Speaker 2: next slide\n\
             (more follows: call get_transcript again with cursor \"2\")\n"
        );
    }

    #[test]
    fn marks_where_the_video_paused() {
        let seg = |id, start_ms, text: &str| Segment {
            id,
            start_ms,
            end_ms: start_ms + 500,
            track: Track::System,
            speaker: Speaker::Remote(1),
            text: text.into(),
            words: vec![],
        };
        let pauses = [
            VideoPause {
                from_ms: 5_000,
                to_ms: 30_000,
                window: WindowView::Hidden,
            },
            VideoPause {
                from_ms: 30_000,
                to_ms: 40_000,
                window: WindowView::Covered,
            },
        ];
        let text = transcript_text(
            "Review",
            "m2",
            &[
                (seg(0, 1_000, "before"), Some(0)),
                (seg(1, 6_000, "during"), Some(0)),
                (seg(2, 9_000, "still during"), Some(0)),
                (seg(3, 31_000, "covered"), Some(0)),
                (seg(4, 50_000, "after"), Some(45_000)),
            ],
            &pauses,
            None,
        );
        assert_eq!(
            text,
            "Transcript of \"Review\" (m2). Times are from the start of the recording.\n\
             -- key frame 0 on screen (from 0:00) --\n\
             [0:01] Speaker 1: before\n\
             -- video paused 0:05–0:30 (the call window was minimized or on another \
             desktop): the key frame on screen may be out of date --\n\
             [0:06] Speaker 1: during\n\
             [0:09] Speaker 1: still during\n\
             -- video paused 0:30–0:40 (the call window was behind other windows): the key \
             frame on screen may be out of date --\n\
             [0:31] Speaker 1: covered\n\
             -- key frame 45000 on screen (from 0:45) --\n\
             [0:50] Speaker 1: after\n\
             (end of transcript)\n"
        );
    }
}
