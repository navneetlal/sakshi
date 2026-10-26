use std::path::Path;

use sakshi_core::Language;
use sakshi_models::gguf::GgufMetadata;

use crate::AsrError;

const PROMPT_DICTIONARY: &str = "asr.rnnt.prompt_dictionary";

/// The language codes an ASR model accepts, from its GGUF header.
///
/// The runtime matches codes exactly and silently auto-detects for anything
/// it doesn't know, so pinned codes are checked here first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageTable {
    /// Accepted codes such as `en-US`, `en`, `hi-IN`, `hi`. Empty for a
    /// single-language model.
    codes: Vec<String>,
}

impl LanguageTable {
    pub fn from_model(path: &Path) -> Result<Self, AsrError> {
        let meta = GgufMetadata::read_file(path).map_err(|source| AsrError::ModelHeader {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self::from_metadata(&meta))
    }

    pub fn from_metadata(meta: &GgufMetadata) -> Self {
        // Entries look like "hi-IN:6"; "auto" is the detection prompt.
        let codes = meta
            .string_array(PROMPT_DICTIONARY)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| entry.rsplit_once(':').map(|(code, _)| code))
            .filter(|code| !code.eq_ignore_ascii_case("auto"))
            .map(str::to_owned)
            .collect();
        Self { codes }
    }

    pub fn codes(&self) -> &[String] {
        &self.codes
    }

    /// The `--language` value for `language`: `auto`, or the model's own
    /// spelling of a pinned code (matched case-insensitively).
    pub fn resolve(&self, language: &Language) -> Result<String, AsrError> {
        let code = match language {
            Language::Auto => return Ok("auto".into()),
            Language::Code(code) => code,
        };
        if self.codes.is_empty() {
            return Err(AsrError::UnsupportedLanguage(format!(
                "this model has no language selection, so {code:?} can't be pinned; use \"auto\""
            )));
        }
        self.codes
            .iter()
            .find(|c| c.eq_ignore_ascii_case(code))
            .cloned()
            .ok_or_else(|| {
                AsrError::UnsupportedLanguage(format!(
                    "the model does not support language {code:?}; supported: {}",
                    self.codes.join(", ")
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(entries: &[&str]) -> LanguageTable {
        let mut meta = GgufMetadata {
            version: 3,
            tensor_count: 0,
            kv: Default::default(),
        };
        meta.kv.insert(
            PROMPT_DICTIONARY.into(),
            sakshi_models::gguf::GgufValue::Array(
                entries
                    .iter()
                    .map(|e| sakshi_models::gguf::GgufValue::String((*e).into()))
                    .collect(),
            ),
        );
        LanguageTable::from_metadata(&meta)
    }

    #[test]
    fn resolves_codes_from_the_prompt_dictionary() {
        let t = table(&["en-US:0", "en:0", "hi-IN:6", "hi:6", "auto:101"]);

        assert_eq!(t.codes(), ["en-US", "en", "hi-IN", "hi"]);
        assert_eq!(t.resolve(&Language::Auto).unwrap(), "auto");
        assert_eq!(t.resolve(&Language::Code("hi".into())).unwrap(), "hi");
        assert_eq!(t.resolve(&Language::Code("HI-in".into())).unwrap(), "hi-IN");
        let err = t.resolve(&Language::Code("ta".into())).unwrap_err();
        assert!(err.to_string().contains("\"ta\""), "{err}");
    }

    #[test]
    fn single_language_models_only_take_auto() {
        let t = table(&[]);
        assert_eq!(t.resolve(&Language::Auto).unwrap(), "auto");
        assert!(t.resolve(&Language::Code("en".into())).is_err());
    }
}
