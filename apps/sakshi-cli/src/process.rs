use std::path::PathBuf;

use clap::Args;
use sakshi_app::{ProcessOptions, Progress, process_meeting};
use sakshi_core::{DataDir, Language, MeetingDir};
use sakshi_pipeline::{Stage, StageEvent};

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
        let defaults = ProcessOptions::default();
        Self {
            folder,
            language: defaults.language,
            from: defaults.from,
            force: defaults.force,
            device: defaults.device,
            asr_model: defaults.asr_model,
            diar_model: defaults.diar_model,
            bleed_threshold: defaults.bleed_threshold,
        }
    }
}

pub fn run(data: &DataDir, args: ProcessArgs) -> anyhow::Result<()> {
    let options = ProcessOptions {
        language: args.language,
        from: args.from,
        force: args.force,
        device: args.device,
        asr_model: args.asr_model,
        diar_model: args.diar_model,
        bleed_threshold: args.bleed_threshold,
    };
    let outcome = process_meeting(data, &args.folder, &options, &|progress| match progress {
        Progress::Importing => eprintln!("copying the recording..."),
        // A recording that never finished, e.g. after a crash.
        Progress::Finalizing => eprintln!("finishing the interrupted recording first"),
        Progress::Stage {
            event: StageEvent::Started { stage },
        } => eprintln!("{}...", stage.as_str()),
        _ => {}
    })?;
    let report = outcome.report;

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
    println!(
        "transcript: {}",
        MeetingDir::new(&args.folder).transcript_json().display()
    );
    println!("meeting id: {}", outcome.meeting.id);
    Ok(())
}
