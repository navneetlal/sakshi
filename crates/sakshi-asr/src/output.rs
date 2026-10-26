use serde::Deserialize;

use crate::{RawTranscript, RawWord};

/// `nemo-speech transcribe --format json` output. Fields Sakshi doesn't use
/// (`file`, `text`, `confidence`) are ignored.
#[derive(Debug, Deserialize)]
struct NemoTranscript {
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    words: Vec<NemoWord>,
}

#[derive(Debug, Deserialize)]
struct NemoWord {
    word: String,
    start: f64,
    end: f64,
    #[serde(default = "full_confidence")]
    confidence: f32,
    /// From 1; absent when no speaker was assigned or not diarizing.
    speaker: Option<u32>,
}

fn full_confidence() -> f32 {
    1.0
}

fn to_ms(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1000.0).round() as u64
    } else {
        0
    }
}

/// Parses `nemo-speech transcribe` JSON into words with millisecond times.
pub fn parse_transcribe_json(json: &str) -> Result<RawTranscript, serde_json::Error> {
    let raw: NemoTranscript = serde_json::from_str(json)?;
    let words = raw
        .words
        .into_iter()
        .filter(|w| !w.word.trim().is_empty())
        .map(|w| {
            let start_ms = to_ms(w.start);
            RawWord {
                start_ms,
                end_ms: to_ms(w.end).max(start_ms),
                text: w.word.trim().to_owned(),
                confidence: w.confidence.clamp(0.0, 1.0),
                speaker: w
                    .speaker
                    .filter(|&s| s >= 1)
                    .and_then(|s| u8::try_from(s).ok()),
            }
        })
        .collect();
    Ok(RawTranscript {
        languages: raw.languages,
        duration_ms: to_ms(raw.duration),
        words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_diarized_english_sample() {
        let t = parse_transcribe_json(include_str!("../tests/data/ami_system.json")).unwrap();

        assert_eq!(t.duration_ms, 60_000);
        assert!(t.languages.is_empty());
        assert_eq!(t.words.len(), 128);
        assert_eq!(
            t.words[0],
            RawWord {
                start_ms: 1280,
                end_ms: 1360,
                text: "From".into(),
                confidence: 1.0,
                speaker: Some(1),
            }
        );
        // Every word in this sample is tagged, with three speakers.
        assert!(t.words.iter().all(|w| w.speaker.is_some()));
        let mut speakers: Vec<u8> = t.words.iter().filter_map(|w| w.speaker).collect();
        speakers.sort_unstable();
        speakers.dedup();
        assert_eq!(speakers, [1, 2, 3]);
        // Times never go backwards within a word or across words.
        assert!(t.words.iter().all(|w| w.end_ms >= w.start_ms));
        assert!(t.words.windows(2).all(|p| p[1].start_ms >= p[0].start_ms));
    }

    #[test]
    fn parses_undiarized_hindi_sample() {
        let t = parse_transcribe_json(include_str!("../tests/data/fleurs_hi_mic.json")).unwrap();

        assert_eq!(t.duration_ms, 10_980);
        assert_eq!(t.words.len(), 21);
        assert_eq!(t.words[0].text, "इसे");
        assert_eq!(t.words[1].text, "मंगलवार");
        assert!(t.words.iter().all(|w| w.speaker.is_none()));
    }

    #[test]
    fn tolerates_missing_and_odd_fields() {
        let t = parse_transcribe_json(
            r#"{"file": "x.wav", "text": "", "duration": 2.5, "languages": ["hi-IN"],
                "words": [
                  {"word": " hi ", "start": 1234.57, "end": 1234.5, "speaker": 0},
                  {"word": "", "start": 1, "end": 2},
                  {"word": "there", "start": -1, "end": 0.08, "confidence": 3, "speaker": 300}
                ]}"#,
        )
        .unwrap();

        assert_eq!(t.languages, ["hi-IN"]);
        assert_eq!(t.words.len(), 2);
        assert_eq!(t.words[0].text, "hi");
        assert_eq!(
            (t.words[0].start_ms, t.words[0].end_ms),
            (1_234_570, 1_234_570)
        );
        assert_eq!(t.words[0].speaker, None);
        assert_eq!(t.words[1].start_ms, 0);
        assert_eq!(t.words[1].confidence, 1.0);
        assert_eq!(t.words[1].speaker, None);
    }

    #[test]
    fn empty_transcript_has_no_words() {
        let t = parse_transcribe_json(
            r#"{"file": "x.wav", "text": "", "confidence": 0, "duration": 4, "languages": [], "words": []}"#,
        )
        .unwrap();
        assert!(t.words.is_empty());
        assert_eq!(t.duration_ms, 4000);
    }
}
