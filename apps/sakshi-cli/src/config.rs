use std::path::PathBuf;

use clap::Subcommand;
use sakshi_app::{check_language, load_settings, save_settings, set_meeting_language};
use sakshi_core::{DataDir, Language, MeetingDir};

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
    /// Turn the "let participants know" reminder before recording on or off.
    ConsentReminder { state: OnOff },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

pub fn run(data: &DataDir, cmd: ConfigCommand) -> anyhow::Result<()> {
    match cmd {
        ConfigCommand::Show => {
            let settings = load_settings(data)?;
            println!("data directory:   {}", data.root().display());
            println!("language:         {}", settings.language);
            let reminder = if settings.consent_reminder {
                "on"
            } else {
                "off"
            };
            println!("consent reminder: {reminder}");
            Ok(())
        }
        ConfigCommand::ConsentReminder { state } => {
            let mut settings = load_settings(data)?;
            settings.consent_reminder = matches!(state, OnOff::On);
            save_settings(data, &settings)?;
            println!(
                "consent reminder {}",
                if settings.consent_reminder {
                    "on"
                } else {
                    "off"
                }
            );
            Ok(())
        }
        ConfigCommand::Language { language, meeting } => {
            check_language(data, &language)?;
            match meeting {
                Some(folder) => {
                    let meeting = MeetingDir::new(folder);
                    set_meeting_language(&meeting, Some(language.clone()))?;
                    println!("{}: language set to {language}", meeting.path().display());
                }
                None => {
                    let mut settings = load_settings(data)?;
                    settings.language = language.clone();
                    save_settings(data, &settings)?;
                    println!("default language set to {language}");
                }
            }
            Ok(())
        }
    }
}
