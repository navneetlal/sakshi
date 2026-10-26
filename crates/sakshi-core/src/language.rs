use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// The language to transcribe in: detected automatically, or pinned to a
/// code such as `hi`, `en-US`, or `pt-BR`.
///
/// Only the code's shape is checked here. Whether a model supports it is
/// checked against that model's language table before transcription, since
/// the runtime silently falls back to detection for codes it doesn't know.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Language {
    #[default]
    Auto,
    Code(String),
}

impl FromStr for Language {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        let (primary, region) = match s.split_once('-') {
            Some((p, r)) => (p, Some(r)),
            None => (s, None),
        };
        let primary_ok =
            (2..=3).contains(&primary.len()) && primary.chars().all(|c| c.is_ascii_alphabetic());
        let region_ok = region.is_none_or(|r| {
            (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric())
        });
        if primary_ok && region_ok {
            Ok(Self::Code(s.to_owned()))
        } else {
            Err(Error::InvalidLanguage(s.to_owned()))
        }
    }
}

impl TryFrom<String> for Language {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Error> {
        s.parse()
    }
}

impl From<Language> for String {
    fn from(language: Language) -> Self {
        language.to_string()
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Code(code) => f.write_str(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auto_and_codes() {
        assert_eq!("auto".parse::<Language>().unwrap(), Language::Auto);
        assert_eq!("AUTO".parse::<Language>().unwrap(), Language::Auto);
        assert_eq!(
            "hi".parse::<Language>().unwrap(),
            Language::Code("hi".into())
        );
        assert_eq!(
            "en-US".parse::<Language>().unwrap(),
            Language::Code("en-US".into())
        );
        assert_eq!(
            "nah-MX".parse::<Language>().unwrap(),
            Language::Code("nah-MX".into())
        );
    }

    #[test]
    fn rejects_malformed_codes() {
        for bad in ["", "e", "english", "en_US", "en-", "en-USAAA", "1n"] {
            assert!(bad.parse::<Language>().is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let json = serde_json::to_string(&Language::Code("hi-IN".into())).unwrap();
        assert_eq!(json, r#""hi-IN""#);
        let back: Language = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(back, Language::Auto);
        assert!(serde_json::from_str::<Language>(r#""xx_yy""#).is_err());
    }
}
