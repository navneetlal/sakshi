//! Joining recognized words into text, and normalizing text for comparison.

/// Punctuation that attaches to the preceding word without a space.
fn is_closing_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | '!'
            | '?'
            | ';'
            | ':'
            | '%'
            | ')'
            | ']'
            | '}'
            | '…'
            | '।'
            | '॥'
            | '、'
            | '。'
            | '，'
            | '！'
            | '？'
            | '：'
            | '；'
    )
}

/// Scripts written without spaces between words.
fn is_unspaced_script(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}'   // Hiragana, Katakana
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{4E00}'..='\u{9FFF}' // CJK Unified Ideographs
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{0E00}'..='\u{0E7F}' // Thai
        | '\u{0E80}'..='\u{0EFF}' // Lao
        | '\u{1780}'..='\u{17FF}' // Khmer
    )
}

/// Joins words with spaces, except before closing punctuation and between
/// words of scripts that don't use spaces.
pub(crate) fn join_words<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for word in words {
        let (Some(first), Some(prev)) = (word.chars().next(), out.chars().next_back()) else {
            out.push_str(word);
            continue;
        };
        if !is_closing_punct(first) && !(is_unspaced_script(prev) && is_unspaced_script(first)) {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Lowercased tokens with punctuation removed, for comparing what two
/// tracks heard.
pub(crate) fn normalize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|&c| {
                    !(c.is_ascii_punctuation()
                        || is_closing_punct(c)
                        || matches!(c, '“' | '”' | '‘' | '’' | '«' | '»' | '¿' | '¡' | '—' | '–'))
                })
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_with_spaces_and_attached_punctuation() {
        assert_eq!(join_words(["Hello", ",", "world", "."]), "Hello, world.");
        assert_eq!(
            join_words(["time,", "not", "by", "speaker"]),
            "time, not by speaker"
        );
        assert_eq!(join_words(["गया", "था", "।"]), "गया था।");
        assert_eq!(join_words(["今日", "は", "晴れ", "。"]), "今日は晴れ。");
        assert_eq!(join_words(["iPhone", "の", "話"]), "iPhone の話");
        assert_eq!(join_words(Vec::<&str>::new()), "");
    }

    #[test]
    fn normalizes_case_punctuation_and_scripts() {
        assert_eq!(normalize("Okay, we're DONE!"), ["okay", "were", "done"]);
        assert_eq!(normalize("“Yes” — no…"), ["yes", "no"]);
        // Devanagari vowel signs and viramas are part of words, not punctuation.
        assert_eq!(normalize("काटना तय किया।"), ["काटना", "तय", "किया"]);
        assert!(normalize(" , . ").is_empty());
    }
}
