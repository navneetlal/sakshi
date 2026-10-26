//! Plain-text rendering of transcripts and times.

use sakshi_core::Segment;

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

/// One line per segment, with a marker whenever a different key frame is on
/// screen.
pub(crate) fn transcript_text(
    title: &str,
    meeting_id: &str,
    segments: &[(Segment, Option<u64>)],
    next_cursor: Option<&str>,
) -> String {
    let mut out = format!(
        "Transcript of \"{title}\" ({meeting_id}). Times are from the start of the recording.\n"
    );
    let mut on_screen = None;
    for (s, frame) in segments {
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
}
