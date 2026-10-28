use std::path::PathBuf;

use clap::Args;
use sakshi_app::{import_folder, request_import};
use sakshi_core::{DataDir, Language};

use crate::process::{self, ProcessArgs};

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// A video or audio file from another recorder, such as a Zoom or
    /// Fireflies recording. It is copied; the original is left alone.
    pub(crate) file: PathBuf,

    /// Meeting title. Defaults to the file name.
    #[arg(long)]
    pub(crate) title: Option<String>,

    /// When the meeting started, in RFC 3339 (e.g. 2026-10-27T10:00:00+05:30).
    /// Defaults to the time stored in the file, if any.
    #[arg(long, value_name = "TIME")]
    pub(crate) started_at: Option<String>,

    /// Transcription language: `auto` or a code such as `hi` or `en-US`.
    #[arg(long)]
    pub(crate) language: Option<Language>,
}

pub fn run(data: &DataDir, args: ImportArgs) -> anyhow::Result<()> {
    let request = request_import(data, &args.file, args.title, args.started_at, args.language)?;
    let meeting = import_folder(data, &request)?;
    eprintln!(
        "importing {} as meeting {}",
        request.file_name(),
        request.id
    );
    process::run(data, ProcessArgs::for_folder(meeting.path().to_path_buf()))
}
