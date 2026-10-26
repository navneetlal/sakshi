//! Removing remote speech that leaked into the microphone.
//!
//! Without headphones, the mic picks up the remote audio from the speakers
//! and the recognizer transcribes it a second time. That echo says nearly
//! the same words as the system track at nearly the same time. When the
//! user speaks right after an echo, both can land in one mic segment, so
//! besides dropping whole segments that match, long matching runs are cut
//! out of segments and the user's own words are kept.

use sakshi_core::{Segment, Track, Word};
use serde::{Deserialize, Serialize};

use crate::text::{join_words, normalize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleedConfig {
    /// Fraction of a mic segment's words that must appear, in order, in the
    /// overlapping system speech for the whole segment to be dropped, in
    /// `0..=1`.
    pub threshold: f32,
    /// How far around a mic segment to look for matching system speech.
    pub window_ms: u64,
    /// Mic segments shorter than this many words are always kept: "yes" or
    /// "okay" match too easily to be judged.
    pub min_words: usize,
    /// A run of at least this many consecutive mic words matching system
    /// speech in order (one misheard word allowed between matches) is cut
    /// out even when the rest of the segment is the user's own speech.
    pub min_run_words: usize,
}

impl Default for BleedConfig {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            window_ms: 2000,
            min_words: 2,
            min_run_words: 4,
        }
    }
}

/// A piece of the mic track removed as bleed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BleedMatch {
    pub segment: Segment,
    /// Share of the removed words that matched system speech.
    pub similarity: f32,
}

/// Splits `segments` into what to keep and the mic speech that is bleed.
/// System segments are always kept; mic segments are kept, dropped, or
/// trimmed into the parts that are the user's own speech. Both lists come
/// back in time order (trimming can move a segment's start later).
pub fn remove_bleed(
    segments: Vec<Segment>,
    config: &BleedConfig,
) -> (Vec<Segment>, Vec<BleedMatch>) {
    let system: Vec<SystemToken> = segments
        .iter()
        .filter(|s| s.track == Track::System)
        .flat_map(|s| &s.words)
        .filter_map(|w| {
            token(w).map(|text| SystemToken {
                start_ms: w.start_ms,
                end_ms: w.end_ms,
                text,
            })
        })
        .collect();

    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for segment in segments {
        if segment.track == Track::System {
            kept.push(segment);
            continue;
        }
        match judge(&segment, &system, config) {
            Verdict::Keep => kept.push(segment),
            Verdict::Drop { similarity } => removed.push(BleedMatch {
                segment,
                similarity,
            }),
            Verdict::Trim { drop, similarity } => {
                for (part, dropped) in split(&segment, &drop) {
                    if dropped {
                        removed.push(BleedMatch {
                            segment: part,
                            similarity,
                        });
                    } else {
                        kept.push(part);
                    }
                }
            }
        }
    }
    let order = |s: &Segment| (s.start_ms, s.end_ms, s.track == Track::System);
    kept.sort_by_key(order);
    removed.sort_by_key(|m| order(&m.segment));
    (kept, removed)
}

struct SystemToken {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

enum Verdict {
    Keep,
    Drop {
        similarity: f32,
    },
    /// `drop[i]` says whether word `i` of the segment is bleed.
    Trim {
        drop: Vec<bool>,
        similarity: f32,
    },
}

/// A word's comparison token, or `None` for punctuation.
fn token(word: &Word) -> Option<String> {
    let t = normalize(&word.text).concat();
    (!t.is_empty()).then_some(t)
}

fn judge(mic: &Segment, system: &[SystemToken], config: &BleedConfig) -> Verdict {
    // (word index, token) for every word that isn't just punctuation.
    let mic_tokens: Vec<(usize, String)> = mic
        .words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| token(w).map(|t| (i, t)))
        .collect();
    if mic_tokens.len() < config.min_words.max(1) {
        return Verdict::Keep;
    }
    let from = mic.start_ms.saturating_sub(config.window_ms);
    let to = mic.end_ms.saturating_add(config.window_ms);
    let nearby: Vec<&str> = system
        .iter()
        .filter(|t| t.end_ms >= from && t.start_ms <= to)
        .map(|t| t.text.as_str())
        .collect();
    if nearby.is_empty() {
        return Verdict::Keep;
    }

    let mic_texts: Vec<&str> = mic_tokens.iter().map(|(_, t)| t.as_str()).collect();
    let matched = lcs_matches(&mic_texts, &nearby);
    let total = matched.iter().filter(|&&m| m).count();
    let similarity = total as f32 / mic_tokens.len() as f32;
    if similarity >= config.threshold {
        return Verdict::Drop { similarity };
    }

    // Runs of matched tokens, bridging single unmatched tokens.
    let mut drop = vec![false; mic.words.len()];
    let (mut run_matched, mut run_tokens) = (0usize, 0usize);
    let mut dropped = (0usize, 0usize);
    let mut run: Option<(usize, usize)> = None; // (first, last) matched token
    let mut gap = 0;
    let mut close = |run: &mut Option<(usize, usize)>, matched: usize, tokens: usize| {
        if let Some((first, last)) = run.take()
            && matched >= config.min_run_words.max(1)
        {
            let (start, end) = (mic_tokens[first].0, mic_tokens[last].0);
            drop[start..=end].iter_mut().for_each(|d| *d = true);
            dropped.0 += matched;
            dropped.1 += tokens;
        }
    };
    for (i, &is_match) in matched.iter().enumerate() {
        if is_match {
            match &mut run {
                Some((_, last)) => *last = i,
                None => {
                    run = Some((i, i));
                    (run_matched, run_tokens) = (0, 0);
                }
            }
            run_tokens += gap + 1;
            run_matched += 1;
            gap = 0;
        } else if run.is_some() {
            gap += 1;
            if gap > 1 {
                close(&mut run, run_matched, run_tokens);
                gap = 0;
            }
        }
    }
    close(&mut run, run_matched, run_tokens);

    if dropped.1 == 0 {
        return Verdict::Keep;
    }
    // Leftovers that are only punctuation belong with the bleed around them.
    for part in parts(&drop) {
        if !part.dropped
            && mic.words[part.range.clone()]
                .iter()
                .all(|w| token(w).is_none())
        {
            drop[part.range].iter_mut().for_each(|d| *d = true);
        }
    }
    Verdict::Trim {
        drop,
        similarity: dropped.0 as f32 / dropped.1 as f32,
    }
}

struct Part {
    range: std::ops::Range<usize>,
    dropped: bool,
}

/// Maximal runs of words with the same `drop` flag.
fn parts(drop: &[bool]) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for (i, &d) in drop.iter().enumerate() {
        match out.last_mut() {
            Some(p) if p.dropped == d => p.range.end = i + 1,
            _ => out.push(Part {
                range: i..i + 1,
                dropped: d,
            }),
        }
    }
    out
}

/// Cuts `segment` into consecutive pieces by `drop`, each a full segment.
fn split(segment: &Segment, drop: &[bool]) -> Vec<(Segment, bool)> {
    parts(drop)
        .into_iter()
        .map(|part| {
            let words = segment.words[part.range].to_vec();
            let piece = Segment {
                id: 0,
                start_ms: words.first().map_or(segment.start_ms, |w| w.start_ms),
                end_ms: words
                    .iter()
                    .map(|w| w.end_ms)
                    .max()
                    .unwrap_or(segment.end_ms),
                track: segment.track,
                speaker: segment.speaker,
                text: join_words(words.iter().map(|w| w.text.as_str())),
                words,
            };
            (piece, part.dropped)
        })
        .collect()
}

/// For each token of `a`, whether it is part of a longest common
/// subsequence of `a` and `b`.
fn lcs_matches(a: &[&str], b: &[&str]) -> Vec<bool> {
    let (n, m) = (a.len(), b.len());
    // table[i][j] = LCS length of a[i..] and b[j..].
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut matched = vec![false; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            matched[i] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use sakshi_core::Speaker;

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

    fn texts(segments: &[Segment], track: Track) -> Vec<&str> {
        segments
            .iter()
            .filter(|s| s.track == track)
            .map(|s| s.text.as_str())
            .collect()
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
        assert_eq!(texts(&kept, Track::Mic), ["sounds good to me"]);
        assert_eq!(texts(&kept, Track::System).len(), 1);
    }

    #[test]
    fn trims_an_echo_followed_by_the_users_own_words() {
        let segments = vec![
            seg(
                Track::System,
                52_000,
                "probably not I mean just get work done",
            ),
            seg(
                Track::Mic,
                52_100,
                "probably not I mean just get work done we can just email if we need",
            ),
        ];

        let (kept, removed) = remove_bleed(segments, &BleedConfig::default());

        assert_eq!(texts(&kept, Track::Mic), ["we can just email if we need"]);
        assert_eq!(removed.len(), 1);
        assert_eq!(
            removed[0].segment.text,
            "probably not I mean just get work done"
        );
        assert_eq!(removed[0].similarity, 1.0);
        let mine = kept.iter().find(|s| s.track == Track::Mic).unwrap();
        assert_eq!(mine.start_ms, mine.words[0].start_ms);
        assert_eq!(mine.words.len(), 7);
    }

    #[test]
    fn trimmed_segments_stay_in_time_order() {
        // Found in a live recording: the mic echo starts before the next
        // system segment, but what's left after trimming starts after it.
        let segments = vec![
            seg(
                Track::Mic,
                14_500,
                "but so I don't know really all it is about",
            ),
            seg(Track::System, 14_600, "but so I don't know really"),
            seg(Track::System, 15_000, "what we're gonna do"),
        ];

        let (kept, removed) = remove_bleed(segments, &BleedConfig::default());

        let starts: Vec<u64> = kept.iter().map(|s| s.start_ms).collect();
        assert!(starts.windows(2).all(|p| p[0] <= p[1]), "{starts:?}");
        assert_eq!(kept.last().unwrap().text, "all it is about");
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn a_run_survives_one_misheard_word() {
        let segments = vec![
            seg(
                Track::System,
                0,
                "yeah we do we need another meeting this week",
            ),
            seg(
                Track::Mic,
                0,
                "for DND yeah we need anther meeting this week",
            ),
        ];

        let (kept, _) = remove_bleed(segments, &BleedConfig::default());

        assert_eq!(texts(&kept, Track::Mic), ["for DND"]);
    }

    #[test]
    fn short_runs_are_left_alone_unless_configured() {
        let segments = || {
            vec![
                seg(Track::System, 0, "see you on thursday"),
                seg(Track::Mic, 0, "great I will see you thursday then bye"),
            ]
        };
        // "see you thursday" is a run of three.
        assert!(removed_texts(segments(), &BleedConfig::default()).is_empty());

        let config = BleedConfig {
            min_run_words: 3,
            ..BleedConfig::default()
        };
        assert_eq!(removed_texts(segments(), &config), ["see you thursday"]);
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
                seg(Track::Mic, 200, "we can email them about lunch plans"),
            ]
        };
        // 3 of 7 mic tokens ("we", "can", "email") match in order: 0.43,
        // with a gap of one between "can" and "email" but only 3 in the run.
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
    fn lcs_marks_in_order_matches() {
        assert_eq!(
            lcs_matches(&["a", "b", "c", "d"], &["a", "x", "c", "d"]),
            [true, false, true, true]
        );
        assert_eq!(
            lcs_matches(&["a", "b", "c"], &["c", "b", "a"])
                .iter()
                .filter(|&&m| m)
                .count(),
            1
        );
        assert_eq!(lcs_matches(&[], &["a"]), Vec::<bool>::new());
    }
}
