use std::path::PathBuf;

use clap::Subcommand;
use sakshi_asr::LanguageTable;
use sakshi_core::{DataDir, Language, MeetingDir, MeetingInfo, read_json, write_json_atomic};
use sakshi_models::{ModelStore, Role};

use crate::process::load_settings;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the current settings.
    Show,
    /// Set the transcription language: `auto`, or a code such as `hi` or
    /// `en-US`. Applies to every meeting, or with --meeting to one.
    Language {
        language: Language,
        /// Meeting folder to pin instead of the global setting.
        #[arg(long, value_name = "FOLDER")]
        meeting: Option<PathBuf>,
    },
}

pub fn run(data: &DataDir, cmd: ConfigCommand) -> anyhow::Result<()> {
    match cmd {
        ConfigCommand::Show => {
            let settings = load_settings(data)?;
            println!("data directory: {}", data.root().display());
            println!("language:       {}", settings.language);
            Ok(())
        }
        ConfigCommand::Language { language, meeting } => {
            check_supported(data, &language)?;
            match meeting {
                Some(folder) => {
                    let meeting = MeetingDir::new(folder);
                    let path = meeting.info_json();
                    let mut info: MeetingInfo = if path.exists() {
                        read_json(&path)?
                    } else {
                        MeetingInfo::default()
                    };
                    info.language = Some(language.clone());
                    write_json_atomic(&path, &info)?;
                    println!("{}: language set to {language}", meeting.path().display());
                }
                None => {
                    let mut settings = load_settings(data)?;
                    settings.language = language.clone();
                    write_json_atomic(&data.config_path(), &settings)?;
                    println!("default language set to {language}");
                }
            }
            Ok(())
        }
    }
}

/// Checks a pinned code against the default ASR model when it is
/// downloaded; otherwise the check happens before transcription.
fn check_supported(data: &DataDir, language: &Language) -> anyhow::Result<()> {
    let store = ModelStore::open(data)?;
    let entry = store.default_for(Role::Asr)?;
    if let Some(path) = store.installed_path(&entry)? {
        LanguageTable::from_model(&path)?.resolve(language)?;
    }
    Ok(())
}
