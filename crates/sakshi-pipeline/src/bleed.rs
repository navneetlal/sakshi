//! Removing remote speech that leaked into the microphone.
//!
//! Without headphones, the mic picks up the remote audio from the speakers
//! and the recognizer transcribes it a second time. Such a mic segment says
//! nearly the same thing as the system track at nearly the same time.

use sakshi_core::{Segment, Track};
use serde::{Deserialize, Serialize};

use crate::text::normalize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleedConfig {
    /// Fraction of a mic segment's words (in order) that must also appear in
    /// the overlapping system speech for it to count as bleed, in `0..=1`.
    pub threshold: f32,
    /// How far around the mic segment to look for matching system speech.
    pub window_ms: u64,
    /// Mic segments shorter than this many words are always kept: "yes" or
    /// "okay" match too easily to be judged.
    pub min_words: usize,
}

impl Default for BleedConfig {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            window_ms: 2000,
            min_words: 2,
        }
    }
}

/// How a removed mic segment matched the system track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleedMatch {
    pub segment: Segment,
    pub similarity: f32,
}

/// Splits `segments` into those to keep and the mic segments that are bleed.
/// System segments are always kept.
pub fn remove_bleed(
    segments: Vec<Segment>,
    config: &BleedConfig,
) -> (Vec<Segment>, Vec<BleedMatch>) {
    let system: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.track == Track::System)
        .collect();
    let mut verdicts = Vec::with_capacity(segments.len());
    for segment in &segments {
        let similarity = match segment.track {
            Track::Mic => similarity(segment, &system, config),
            Track::System => None,
        };
        verdicts.push(similarity.filter(|&s| s >= config.threshold));
    }

    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for (segment, verdict) in segments.into_iter().zip(verdicts) {
        match verdict {
            Some(similarity) => removed.push(BleedMatch {
                segment,
                similarity,
            }),
            None => kept.push(segment),
        }
    }
    (kept, removed)
}

/// The share of the mic segment's tokens found, in order, in system speech
/// within the window; `None` if the segment is too short to judge.
fn similarity(mic: &Segment, system: &[&Segment], config: &BleedConfig) -> Option<f32> {
    let mic_tokens = normalize(&mic.text);
    if mic_tokens.len() < config.min_words.max(1) {
        return None;
    }
    let from = mic.start_ms.saturating_sub(config.window_ms);
    let to = mic.end_ms.saturating_add(config.window_ms);
    let nearby: String = system
        .iter()
        .flat_map(|s| &s.words)
        .filter(|w| w.end_ms >= from && w.start_ms <= to)
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let system_tokens = normalize(&nearby);
    if system_tokens.is_empty() {
        return Some(0.0);
    }
    Some(lcs_len(&mic_tokens, &system_tokens) as f32 / mic_tokens.len() as f32)
}

/// Length of the longest common subsequence of two token lists.
fn lcs_len(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use sakshi_core::{Speaker, Word};

    use super::*;

    fn seg(track: Track, start_ms: u64, text: &str) -> Segment {
        let words: Vec<Word> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| Word {
                start_ms: start_ms + i as u64 * 300,
                end_ms: start_ms + i as u64 * 300 + 250,
                text: t.into(),
                p: 1.0,
            })
            .collect();
        Segment {
            id: 0,
            start_ms,
            end_ms: words.last().map_or(start_ms, |w| w.end_ms),
            track,
            speaker: match track {
                Track::Mic => Speaker::You,
                Track::System => Speaker::Remote(1),
            },
            text: text.into(),
            words,
        }
    }

    fn removed_texts(segments: Vec<Segment>, config: &BleedConfig) -> Vec<String> {
        remove_bleed(segments, config)
            .1
            .into_iter()
            .map(|m| m.segment.text)
            .collect()
    }

    #[test]
    fn drops_mic_echo_of_system_speech() {
        let segments = vec![
            seg(Track::System, 10_000, "I guess we just see what Steve says"),
            // The same words, slightly late and partly misheard.
            seg(Track::Mic, 10_150, "I guess we just see what Steve said"),
            // The user's own reply.
            seg(Track::Mic, 14_000, "sounds good to me"),
        ];

        let (kept, removed) = remove_bleed(segments, &BleedConfig::default());

        assert_eq!(removed.len(), 1);
        assert!((removed[0].similarity - 7.0 / 8.0).abs() < 1e-6);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|s| s.text == "sounds good to me"));
    }

    #[test]
    fn keeps_matching_text_outside_the_window() {
        let segments = vec![
            seg(Track::System, 0, "let us meet on thursday"),
            seg(Track::Mic, 30_000, "let us meet on thursday"),
        ];
        assert!(removed_texts(segments, &BleedConfig::default()).is_empty());
    }

    #[test]
    fn keeps_short_mic_segments() {
        let segments = vec![seg(Track::System, 0, "yeah"), seg(Track::Mic, 100, "yeah")];
        assert!(removed_texts(segments, &BleedConfig::default()).is_empty());
    }

    #[test]
    fn threshold_is_configurable() {
        let segments = || {
            vec![
                seg(Track::System, 0, "we can just email if we need anything"),
                // Half of these words appear in the system speech.
                seg(Track::Mic, 200, "we can email them about lunch plans"),
            ]
        };
        // 3 of 7 mic tokens ("we", "can", "email") match in order: 0.43.
        let strict = BleedConfig::default();
        assert!(removed_texts(segments(), &strict).is_empty());

        let loose = BleedConfig {
            threshold: 0.4,
            ..BleedConfig::default()
        };
        assert_eq!(removed_texts(segments(), &loose).len(), 1);
    }

    #[test]
    fn matching_ignores_case_and_punctuation() {
        let segments = vec![
            seg(Track::System, 0, "Okay, are we done?"),
            seg(Track::Mic, 0, "okay are we done"),
        ];
        assert_eq!(removed_texts(segments, &BleedConfig::default()).len(), 1);
    }

    #[test]
    fn works_for_hindi() {
        let segments = vec![
            seg(Track::System, 0, "इसे मंगलवार को काटना तय किया गया था।"),
            seg(Track::Mic, 120, "इसे मंगलवार को काटना तय किया था"),
        ];
        assert_eq!(removed_texts(segments, &BleedConfig::default()).len(), 1);
    }

    #[test]
    fn lcs_counts_in_order_matches() {
        let t = |s: &str| normalize(s);
        assert_eq!(lcs_len(&t("a b c d"), &t("a x c d")), 3);
        assert_eq!(lcs_len(&t("a b c"), &t("c b a")), 1);
        assert_eq!(lcs_len(&t(""), &t("a")), 0);
    }
}
