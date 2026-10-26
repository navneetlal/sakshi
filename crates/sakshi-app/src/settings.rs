use sakshi_asr::LanguageTable;
use sakshi_core::{
    DataDir, Language, MeetingDir, MeetingInfo, Settings, read_json, write_json_atomic,
};
use sakshi_models::{ModelStore, Role};

use crate::AppError;

/// `config.json`, or the defaults if it doesn't exist yet.
pub fn load_settings(data: &DataDir) -> Result<Settings, AppError> {
    let path = data.config_path();
    if !path.exists() {
        return Ok(Settings::default());
    }
    Ok(read_json(&path)?)
}

pub fn save_settings(data: &DataDir, settings: &Settings) -> Result<(), AppError> {
    Ok(write_json_atomic(&data.config_path(), settings)?)
}

/// The language for a meeting: `explicit` if given, else the meeting's
/// `meeting.json`, else the global setting.
pub fn resolve_language(
    data: &DataDir,
    meeting: &MeetingDir,
    explicit: Option<Language>,
) -> Result<Language, AppError> {
    if let Some(language) = explicit {
        return Ok(language);
    }
    if meeting.info_json().exists() {
        let info: MeetingInfo = read_json(&meeting.info_json())?;
        if let Some(language) = info.language {
            return Ok(language);
        }
    }
    Ok(load_settings(data)?.language)
}

/// Pins one meeting's language in its `meeting.json`, or with `None` lets
/// it follow the global setting again.
pub fn set_meeting_language(
    meeting: &MeetingDir,
    language: Option<Language>,
) -> Result<(), AppError> {
    let path = meeting.info_json();
    let mut info: MeetingInfo = if path.exists() {
        read_json(&path)?
    } else {
        MeetingInfo::default()
    };
    info.language = language;
    Ok(write_json_atomic(&path, &info)?)
}

/// The language codes the default ASR model accepts, or `None` until it's
/// downloaded.
pub fn supported_languages(data: &DataDir) -> Result<Option<Vec<String>>, AppError> {
    let store = ModelStore::open(data)?;
    let entry = store.default_for(Role::Asr)?;
    match store.installed_path(&entry)? {
        Some(path) => Ok(Some(LanguageTable::from_model(&path)?.codes().to_vec())),
        None => Ok(None),
    }
}

/// Checks a pinned language against the default ASR model when it's
/// downloaded; otherwise the check happens before transcription.
pub fn check_language(data: &DataDir, language: &Language) -> Result<(), AppError> {
    let store = ModelStore::open(data)?;
    let entry = store.default_for(Role::Asr)?;
    if let Some(path) = store.installed_path(&entry)? {
        LanguageTable::from_model(&path)?.resolve(language)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_precedence_is_explicit_meeting_global() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path().join("data"));
        let meeting = MeetingDir::new(root.path().join("m"));
        let code = |c: &str| Language::Code(c.into());

        assert_eq!(
            resolve_language(&data, &meeting, None).unwrap(),
            Language::Auto
        );

        let settings = Settings {
            language: code("en"),
            ..Settings::default()
        };
        save_settings(&data, &settings).unwrap();
        assert_eq!(resolve_language(&data, &meeting, None).unwrap(), code("en"));

        let info = MeetingInfo {
            language: Some(code("hi")),
            ..MeetingInfo::default()
        };
        write_json_atomic(&meeting.info_json(), &info).unwrap();
        assert_eq!(resolve_language(&data, &meeting, None).unwrap(), code("hi"));
        assert_eq!(
            resolve_language(&data, &meeting, Some(Language::Auto)).unwrap(),
            Language::Auto
        );
    }
}
