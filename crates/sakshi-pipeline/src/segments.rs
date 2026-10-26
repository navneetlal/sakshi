//! Building transcript segments from recognized words.

use sakshi_asr::RawWord;
use sakshi_core::{Segment, Speaker, Track, Word};

use crate::text::join_words;

/// Splits one track's words into segments: a new segment starts whenever
/// the speaker changes or the pause before a word exceeds `gap_ms`.
///
/// `mic` words are all "You". `system` words carry the diarizer's speaker
/// index; words it left untagged take the speaker of the nearest tagged
/// word, preferring the previous one when the pause is short. Indices are
/// renumbered to "Speaker 1", "Speaker 2", … in order of first appearance.
/// Segment ids are left at 0 for the caller to number.
pub fn build_segments(words: &[RawWord], track: Track, gap_ms: u64) -> Vec<Segment> {
    let speakers: Vec<Speaker> = match track {
        Track::Mic => vec![Speaker::You; words.len()],
        Track::System => remote_speakers(words, gap_ms),
    };

    let mut segments: Vec<Segment> = Vec::new();
    for (word, speaker) in words.iter().zip(speakers) {
        let continues = segments.last().is_some_and(|s| {
            s.speaker == speaker && word.start_ms.saturating_sub(s.end_ms) <= gap_ms
        });
        if !continues {
            segments.push(Segment {
                id: 0,
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                track,
                speaker,
                text: String::new(),
                words: Vec::new(),
            });
        }
        let segment = segments.last_mut().expect("pushed above");
        segment.end_ms = segment.end_ms.max(word.end_ms);
        segment.words.push(Word {
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            text: word.text.clone(),
            p: word.confidence,
        });
    }
    for segment in &mut segments {
        segment.text = join_words(segment.words.iter().map(|w| w.text.as_str()));
    }
    segments
}

/// Speaker for every system word, numbered in order of first appearance.
fn remote_speakers(words: &[RawWord], gap_ms: u64) -> Vec<Speaker> {
    let filled = fill_untagged(words, gap_ms);
    let mut order: Vec<u8> = Vec::new();
    filled
        .into_iter()
        .map(|raw| {
            let n = match order.iter().position(|&r| r == raw) {
                Some(i) => i,
                None => {
                    order.push(raw);
                    order.len() - 1
                }
            };
            Speaker::Remote(u8::try_from(n + 1).unwrap_or(u8::MAX))
        })
        .collect()
}

/// The diarizer's speaker index per word, with untagged words given the
/// previous tagged word's speaker if it is within `gap_ms`, else the next
/// one's if within `gap_ms`, else whichever exists. If nothing is tagged
/// at all, everyone is speaker 1.
fn fill_untagged(words: &[RawWord], gap_ms: u64) -> Vec<u8> {
    let mut prev: Vec<Option<(u8, u64)>> = Vec::with_capacity(words.len());
    let mut last = None;
    for w in words {
        if let Some(s) = w.speaker {
            last = Some((s, w.end_ms));
        }
        prev.push(last);
    }
    let mut next: Vec<Option<(u8, u64)>> = vec![None; words.len()];
    let mut upcoming = None;
    for (i, w) in words.iter().enumerate().rev() {
        if let Some(s) = w.speaker {
            upcoming = Some((s, w.start_ms));
        }
        next[i] = upcoming;
    }

    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if let Some(s) = w.speaker {
                return s;
            }
            let before = prev[i].filter(|&(_, end)| w.start_ms.saturating_sub(end) <= gap_ms);
            let after = next[i].filter(|&(_, start)| start.saturating_sub(w.end_ms) <= gap_ms);
            before
                .or(after)
                .or(prev[i])
                .or(next[i])
                .map_or(1, |(s, _)| s)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start_ms: u64, end_ms: u64, text: &str, speaker: Option<u8>) -> RawWord {
        RawWord {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: 1.0,
            speaker,
        }
    }

    fn summary(segments: &[Segment]) -> Vec<(String, u64, u64, String)> {
        segments
            .iter()
            .map(|s| (s.speaker.to_string(), s.start_ms, s.end_ms, s.text.clone()))
            .collect()
    }

    #[test]
    fn splits_on_speaker_changes_and_long_pauses() {
        let words = [
            w(0, 300, "hello", Some(3)),
            w(400, 700, "there.", Some(3)),
            w(800, 1000, "hi", Some(1)),
            w(1100, 1400, "back", Some(1)),
            w(2600, 2900, "later", Some(1)), // 1.2 s pause
            w(3000, 3200, "again", Some(3)),
        ];

        let segments = build_segments(&words, Track::System, 1000);

        assert_eq!(
            summary(&segments),
            [
                ("Speaker 1".into(), 0, 700, "hello there.".into()),
                ("Speaker 2".into(), 800, 1400, "hi back".into()),
                ("Speaker 2".into(), 2600, 2900, "later".into()),
                ("Speaker 1".into(), 3000, 3200, "again".into()),
            ]
        );
        assert!(segments.iter().all(|s| s.track == Track::System));
        assert_eq!(segments[1].words.len(), 2);
    }

    #[test]
    fn mic_words_are_always_you() {
        let words = [w(0, 200, "yes", None), w(300, 500, "please", Some(2))];
        let segments = build_segments(&words, Track::Mic, 1000);
        assert_eq!(
            summary(&segments),
            [("You".into(), 0, 500, "yes please".into())]
        );
    }

    #[test]
    fn untagged_words_join_a_nearby_speaker() {
        let words = [
            w(0, 200, "one", Some(5)),
            w(300, 400, "two", None),     // 100 ms after speaker 5
            w(3000, 3100, "three", None), // far from 5, 100 ms before 7
            w(3200, 3400, "four", Some(7)),
            w(9000, 9100, "five", None), // far from everyone: previous wins
        ];

        let segments = build_segments(&words, Track::System, 1000);

        assert_eq!(
            summary(&segments),
            [
                ("Speaker 1".into(), 0, 400, "one two".into()),
                ("Speaker 2".into(), 3000, 3400, "three four".into()),
                ("Speaker 2".into(), 9000, 9100, "five".into()),
            ]
        );
    }

    #[test]
    fn no_tags_at_all_means_one_remote_speaker() {
        let words = [w(0, 100, "a", None), w(200, 300, "b", None)];
        let segments = build_segments(&words, Track::System, 1000);
        assert_eq!(
            summary(&segments),
            [("Speaker 1".into(), 0, 300, "a b".into())]
        );
    }

    #[test]
    fn empty_input_gives_no_segments() {
        assert!(build_segments(&[], Track::System, 1000).is_empty());
    }
}
