use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::Args;
use sakshi_asr::{NemoSpeech, locate_sidecar};
use sakshi_core::{DataDir, Language, MeetingDir, MeetingInfo, Settings, read_json};
use sakshi_media::Ffmpeg;
use sakshi_models::{ModelStore, Role};
use sakshi_pipeline::{BleedConfig, Pipeline, PipelineConfig, RunOptions, Stage};
use sakshi_store::Store;

#[derive(Debug, Args)]
pub struct ProcessArgs {
    /// Meeting folder containing system.wav and/or mic.wav, and optionally
    /// recording.mp4.
    pub(crate) folder: PathBuf,

    /// Transcription language: `auto` or a code such as `hi` or `en-US`.
    /// Overrides meeting.json and the global setting.
    #[arg(long)]
    pub(crate) language: Option<Language>,

    /// Run this stage again, and every stage that depends on it.
    #[arg(long, value_name = "STAGE")]
    pub(crate) from: Option<Stage>,

    /// Run every stage again.
    #[arg(long)]
    pub(crate) force: bool,

    /// Inference device: auto, cpu, metal, cuda[:N], or vulkan[:N].
    #[arg(long, default_value = "auto")]
    pub(crate) device: String,

    /// ASR model id (see `models list`). Defaults to the bundled default.
    #[arg(long, value_name = "ID")]
    pub(crate) asr_model: Option<String>,

    /// Diarization model id. Defaults to the bundled default.
    #[arg(long, value_name = "ID")]
    pub(crate) diar_model: Option<String>,

    /// Share of a mic segment's words that must match overlapping system
    /// speech for it to be dropped as bleed (0 to 1).
    #[arg(long, value_name = "FRACTION")]
    pub(crate) bleed_threshold: Option<f32>,
}

impl ProcessArgs {
    /// Default settings for one folder.
    pub(crate) fn for_folder(folder: PathBuf) -> Self {
        Self {
            folder,
            language: None,
            from: None,
            force: false,
            device: "auto".into(),
            asr_model: None,
            diar_model: None,
            bleed_threshold: None,
        }
    }
}

pub fn run(data: &DataDir, args: ProcessArgs) -> anyhow::Result<()> {
    if !args.folder.is_dir() {
        bail!("{} is not a folder", args.folder.display());
    }
    let meeting = MeetingDir::new(&args.folder);
    if sakshi_capture::needs_finalize(&meeting) {
        // A recording that never finished, e.g. after a crash.
        println!("finishing the interrupted recording first");
        sakshi_capture::finalize(&meeting, &Ffmpeg::locate()?, false)?;
    }
    let language = resolve_language(data, &meeting, args.language)?;

    let store = ModelStore::open(data)?;
    let model_path = |id: Option<String>, role| -> anyhow::Result<PathBuf> {
        let entry = match id {
            Some(id) => store.find(&id)?,
            None => store.default_for(role)?,
        };
        if entry.role != role {
            bail!("{} is a {} model, not {role}", entry.id, entry.role);
        }
        Ok(store.require(&entry)?)
    };
    let asr = model_path(args.asr_model, Role::Asr)?;
    let diar = model_path(args.diar_model, Role::Diarization)?;

    let transcriber = NemoSpeech::new(locate_sidecar()?, asr, Some(diar))?.with_device(args.device);
    let mut config = PipelineConfig {
        language,
        ..PipelineConfig::default()
    };
    if let Some(threshold) = args.bleed_threshold {
        if !(0.0..=1.0).contains(&threshold) {
            bail!("--bleed-threshold must be between 0 and 1");
        }
        config.bleed = BleedConfig {
            threshold,
            ..config.bleed
        };
    }
    let ffmpeg = Ffmpeg::locate().ok();

    let report = Pipeline::new(&transcriber, ffmpeg, config).run(
        &meeting,
        &RunOptions {
            from: args.from,
            force: args.force,
        },
    )?;

    let names = |stages: &[Stage]| {
        if stages.is_empty() {
            "none".to_owned()
        } else {
            stages
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    println!("ran:        {}", names(&report.ran));
    println!("reused:     {}", names(&report.skipped));
    println!(
        "segments:   {} ({} mic segments dropped as bleed)",
        report.segments, report.bleed_removed
    );
    println!("key frames: {}", report.frames);
    println!(
        "languages:  {}",
        if report.languages.is_empty() {
            "auto-detected (not reported by the engine)".to_owned()
        } else {
            report.languages.join(", ")
        }
    );
    println!("transcript: {}", meeting.transcript_json().display());

    // Make the meeting visible to the MCP server.
    let record = Store::open(&data.db_path())?.index_meeting(&meeting)?;
    println!("meeting id: {}", record.id);
    Ok(())
}

/// `--language`, else the meeting's `meeting.json`, else the global setting.
fn resolve_language(
    data: &DataDir,
    meeting: &MeetingDir,
    flag: Option<Language>,
) -> anyhow::Result<Language> {
    if let Some(language) = flag {
        return Ok(language);
    }
    if meeting.info_json().exists() {
        let info: MeetingInfo = read_json(&meeting.info_json())?;
        if let Some(language) = info.language {
            return Ok(language);
        }
    }
    load_settings(data).map(|s| s.language)
}

pub fn load_settings(data: &DataDir) -> anyhow::Result<Settings> {
    let path = data.config_path();
    if !path.exists() {
        return Ok(Settings::default());
    }
    read_json(&path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sakshi_core::write_json_atomic;

    #[test]
    fn language_precedence_is_flag_meeting_global() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::new(root.path().join("data"));
        let meeting = MeetingDir::new(root.path().join("m"));
        let code = |c: &str| Language::Code(c.into());

        assert_eq!(
            resolve_language(&data, &meeting, None).unwrap(),
            Language::Auto
        );

        write_json_atomic(
            &data.config_path(),
            &Settings {
                language: code("en"),
                ..Settings::default()
            },
        )
        .unwrap();
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
