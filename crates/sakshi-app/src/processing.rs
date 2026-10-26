use std::path::{Path, PathBuf};

use sakshi_asr::{NemoSpeech, locate_sidecar};
use sakshi_core::{DataDir, Language, MeetingDir};
use sakshi_media::Ffmpeg;
use sakshi_models::{ModelStore, Role};
use sakshi_pipeline::{
    BleedConfig, Pipeline, PipelineConfig, Report, RunOptions, Stage, StageEvent,
};
use sakshi_store::{MeetingRecord, Store};
use serde::Serialize;

use crate::{AppError, resolve_language};

#[derive(Debug, Clone)]
pub struct ProcessOptions {
    /// Overrides `meeting.json` and the global setting.
    pub language: Option<Language>,
    pub from: Option<Stage>,
    pub force: bool,
    /// `auto`, `cpu`, `metal`, `cuda[:N]`, or `vulkan[:N]`.
    pub device: String,
    pub asr_model: Option<String>,
    pub diar_model: Option<String>,
    pub bleed_threshold: Option<f32>,
}

impl Default for ProcessOptions {
    fn default() -> Self {
        Self {
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

/// What [`process_meeting`] is doing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Progress {
    /// Turning an unfinished capture into recording.mp4 and the WAVs.
    Finalizing,
    Stage {
        event: StageEvent,
    },
    /// Adding the meeting to the index.
    Indexing,
}

#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub report: Report,
    pub meeting: MeetingRecord,
}

fn model_path(store: &ModelStore, id: Option<&str>, role: Role) -> Result<PathBuf, AppError> {
    let entry = match id {
        Some(id) => store.find(id)?,
        None => store.default_for(role)?,
    };
    if entry.role != role {
        return Err(AppError::Invalid(format!(
            "{} is a {} model, not {role}",
            entry.id, entry.role
        )));
    }
    Ok(store.require(&entry)?)
}

/// Processes a meeting folder end to end: finishes an interrupted capture,
/// runs the pipeline (resuming finished stages), and indexes the result so
/// the MCP server can see it.
pub fn process_meeting(
    data: &DataDir,
    folder: &Path,
    options: &ProcessOptions,
    progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<ProcessOutcome, AppError> {
    if !folder.is_dir() {
        return Err(AppError::Invalid(format!(
            "{} is not a folder",
            folder.display()
        )));
    }
    let meeting = MeetingDir::new(folder);
    if sakshi_capture::needs_finalize(&meeting) {
        progress(Progress::Finalizing);
        sakshi_capture::finalize(&meeting, &Ffmpeg::locate()?, false)?;
    }
    let language = resolve_language(data, &meeting, options.language.clone())?;

    let models = ModelStore::open(data)?;
    let asr = model_path(&models, options.asr_model.as_deref(), Role::Asr)?;
    let diar = model_path(&models, options.diar_model.as_deref(), Role::Diarization)?;
    let transcriber =
        NemoSpeech::new(locate_sidecar()?, asr, Some(diar))?.with_device(options.device.clone());

    let mut config = PipelineConfig {
        language,
        ..PipelineConfig::default()
    };
    if let Some(threshold) = options.bleed_threshold {
        if !(0.0..=1.0).contains(&threshold) {
            return Err(AppError::Invalid(
                "the bleed threshold must be between 0 and 1".into(),
            ));
        }
        config.bleed = BleedConfig {
            threshold,
            ..config.bleed
        };
    }
    let observer = |event: &StageEvent| {
        progress(Progress::Stage {
            event: event.clone(),
        })
    };
    let report = Pipeline::new(&transcriber, Ffmpeg::locate().ok(), config)
        .with_observer(&observer)
        .run(
            &meeting,
            &RunOptions {
                from: options.from,
                force: options.force,
            },
        )?;

    progress(Progress::Indexing);
    let record = Store::open(&data.db_path())?.index_meeting(&meeting)?;
    Ok(ProcessOutcome {
        report,
        meeting: record,
    })
}
