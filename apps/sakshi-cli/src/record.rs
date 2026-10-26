use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Args;
use sakshi_capture::{Capture, CaptureConfig, PermissionState, Source, SourceId};
use sakshi_core::{DataDir, MeetingDir, MeetingInfo, Settings, write_json_atomic};
use sakshi_media::Ffmpeg;

use crate::process::{self, ProcessArgs, load_settings};

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// List the windows and displays that can be recorded, then exit.
    #[arg(long)]
    list: bool,

    /// Record this window (an id from --list).
    #[arg(long, value_name = "ID", conflicts_with_all = ["display", "app"])]
    window: Option<u32>,

    /// Record this whole display (an id from --list).
    #[arg(long, value_name = "ID", conflicts_with = "app")]
    display: Option<u32>,

    /// Record the first window whose app name or title contains this text.
    #[arg(long, value_name = "TEXT")]
    app: Option<String>,

    /// Meeting title (defaults to the window title).
    #[arg(long)]
    title: Option<String>,

    /// Don't record the microphone.
    #[arg(long)]
    no_mic: bool,

    /// Don't record the app's audio.
    #[arg(long)]
    no_system_audio: bool,

    /// Video frame rate.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u32).range(1..=60))]
    fps: u32,

    /// Largest video size, as WIDTHxHEIGHT.
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    max_size: (u32, u32),

    /// Stop after this many seconds instead of waiting for Ctrl-C.
    #[arg(long, value_name = "SECONDS")]
    duration: Option<u64>,

    /// Only record; don't transcribe afterwards.
    #[arg(long)]
    no_process: bool,

    /// Keep the raw capture files after building recording.mp4.
    #[arg(long)]
    keep_raw: bool,

    /// Skip the consent reminder (you've told the participants).
    #[arg(long)]
    yes: bool,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("{s:?} is not WIDTHxHEIGHT"))?;
    let parse = |v: &str| v.trim().parse::<u32>().ok().filter(|&n| n >= 16);
    match (parse(w), parse(h)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err(format!("{s:?} is not WIDTHxHEIGHT (at least 16x16)")),
    }
}

pub fn run(data: &DataDir, args: RecordArgs) -> anyhow::Result<()> {
    let capture = sakshi_capture::platform();
    check_screen_permission(capture.as_ref())?;

    let sources = capture.sources()?;
    if args.list {
        print_sources(&sources);
        return Ok(());
    }
    let source = choose_source(&sources, &args)?;
    let microphone = !args.no_mic;
    if microphone {
        check_microphone_permission(capture.as_ref())?;
    }

    let mut settings = load_settings(data)?;
    if settings.consent_reminder && !args.yes && !remind_about_consent(data, &mut settings)? {
        println!("Not recording.");
        return Ok(());
    }

    let ffmpeg = Ffmpeg::locate()?;
    let (meeting, info) = create_meeting(data, &source, args.title.clone())?;
    let config = CaptureConfig {
        max_width: args.max_size.0,
        max_height: args.max_size.1,
        fps: args.fps,
        microphone,
        system_audio: !args.no_system_audio,
        ..CaptureConfig::default()
    };

    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .context("installing the Ctrl-C handler")?;

    let recording = capture.start(source.id, &config, &meeting, &ffmpeg)?;
    let title = info.title.as_deref().unwrap_or("meeting");
    let live = std::io::stderr().is_terminal();
    eprintln!("Recording \"{title}\" into {}", meeting.path().display());
    eprintln!("Press Ctrl-C to stop.");
    let limit = args.duration.map(Duration::from_secs);
    let reason = loop {
        std::thread::sleep(Duration::from_millis(250));
        let elapsed = recording.elapsed();
        if live {
            eprint!("\r● {}  ", clock(elapsed));
            let _ = std::io::stderr().flush();
        }
        if stop.load(Ordering::SeqCst) {
            break "stopped";
        }
        if limit.is_some_and(|l| elapsed >= l) {
            break "time limit reached";
        }
        if let Some(why) = recording.interruption() {
            eprintln!();
            eprintln!("Capture ended: {why}");
            break "capture ended";
        }
    };
    if live {
        eprintln!();
    }
    let stats = recording.stop()?;
    eprintln!(
        "{reason} after {} ({} video frames)",
        clock(Duration::from_millis(stats.duration_ms)),
        stats.video_frames
    );

    sakshi_capture::finalize(&meeting, &ffmpeg, args.keep_raw)?;
    println!("recording: {}", meeting.recording_mp4().display());
    if args.no_process {
        println!(
            "Not processed. Run `sakshi-cli process {}` later.",
            meeting.path().display()
        );
        return Ok(());
    }
    process::run(data, ProcessArgs::for_folder(meeting.path().to_path_buf()))
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn check_screen_permission(capture: &dyn Capture) -> anyhow::Result<()> {
    if capture.permissions().screen == PermissionState::Granted
        || capture.request_screen_permission()
    {
        return Ok(());
    }
    bail!(
        "Sakshi needs Screen Recording access to record a window.\n\
         Open System Settings > Privacy & Security > Screen & System Audio Recording,\n\
         turn it on for the app you run sakshi-cli from (e.g. Terminal), then quit and\n\
         reopen that app. To open the page directly:\n\
         \n    open \"x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture\""
    )
}

fn check_microphone_permission(capture: &dyn Capture) -> anyhow::Result<()> {
    match capture.request_microphone_permission() {
        PermissionState::Granted => Ok(()),
        _ => bail!(
            "Sakshi needs Microphone access to record your side of the call.\n\
             Open System Settings > Privacy & Security > Microphone and turn it on for the\n\
             app you run sakshi-cli from, or record without it using --no-mic. To open the page:\n\
             \n    open \"x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone\""
        ),
    }
}

fn print_sources(sources: &[Source]) {
    if sources.is_empty() {
        println!("Nothing to record.");
        return;
    }
    for s in sources {
        let marker = if s.meeting.is_some() { "*" } else { " " };
        let (kind, id) = match s.id {
            SourceId::Window(id) => ("--window", id),
            SourceId::Display(id) => ("--display", id),
        };
        let app = s.app_name.as_deref().unwrap_or("");
        let what = match &s.meeting {
            Some(m) => format!("{m}: {}", s.title),
            None if app.is_empty() => s.title.clone(),
            None => format!("{app}: {}", s.title),
        };
        println!("{marker} {kind} {id:<8} {what}  ({}x{})", s.width, s.height);
    }
    if sources.iter().any(|s| s.meeting.is_some()) {
        println!("\n* looks like a meeting");
    }
}

fn choose_source(sources: &[Source], args: &RecordArgs) -> anyhow::Result<Source> {
    let wanted = match (args.window, args.display) {
        (Some(id), _) => Some(SourceId::Window(id)),
        (_, Some(id)) => Some(SourceId::Display(id)),
        _ => None,
    };
    if let Some(id) = wanted {
        return sources
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no {id}; run `sakshi-cli record --list`"));
    }
    if let Some(text) = &args.app {
        let text = text.to_lowercase();
        return sources
            .iter()
            .filter(|s| matches!(s.id, SourceId::Window(_)))
            .find(|s| {
                s.app_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&text)
                    || s.title.to_lowercase().contains(&text)
            })
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("no window matches {text:?}; run `sakshi-cli record --list`")
            });
    }

    // Interactive: likely meetings first, with the first one as the default.
    if !std::io::stdin().is_terminal() {
        bail!("choose what to record with --window, --display, or --app (see --list)");
    }
    let shown: Vec<&Source> = sources.iter().take(20).collect();
    for (i, s) in shown.iter().enumerate() {
        let label = s
            .meeting
            .as_deref()
            .or(s.app_name.as_deref())
            .unwrap_or("Display");
        println!("{:>2}. {label}: {}", i + 1, s.title);
    }
    let default = shown.first().filter(|s| s.meeting.is_some()).map(|_| 1);
    match default {
        Some(n) => print!("Record which? [{n}] "),
        None => print!("Record which? "),
    }
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let n = match answer.trim() {
        "" => default.context("no choice made")?,
        n => n.parse::<usize>().context("enter a number from the list")?,
    };
    shown
        .get(n.wrapping_sub(1))
        .map(|s| (*s).clone())
        .context("enter a number from the list")
}

/// Returns whether to go ahead. "a" also turns the reminder off for good.
fn remind_about_consent(data: &DataDir, settings: &mut Settings) -> anyhow::Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!(
            "Let the participants know this call is being recorded, then pass --yes \
             (or turn the reminder off with `sakshi-cli config consent-reminder off`)."
        );
    }
    println!("Let participants know this call is being recorded.");
    print!("Start recording? [Y]es / [n]o / [a]lways (don't remind me again) ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    match answer.trim().to_lowercase().as_str() {
        "" | "y" | "yes" => Ok(true),
        "a" | "always" => {
            settings.consent_reminder = false;
            write_json_atomic(&data.config_path(), &*settings)?;
            println!(
                "Won't remind you again (turn it back on with `sakshi-cli config consent-reminder on`)."
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// A new meeting folder under the data directory, with its meeting.json.
fn create_meeting(
    data: &DataDir,
    source: &Source,
    title: Option<String>,
) -> anyhow::Result<(MeetingDir, MeetingInfo)> {
    let id = uuid::Uuid::now_v7().to_string();
    let meeting = data.meeting(&id);
    std::fs::create_dir_all(meeting.path())
        .with_context(|| format!("creating {}", meeting.path().display()))?;
    let title = title.or_else(|| {
        Some(match (&source.meeting, source.title.trim()) {
            (Some(app), "") => app.clone(),
            (_, "") => source
                .app_name
                .clone()
                .unwrap_or_else(|| "Recording".into()),
            (_, t) => t.to_owned(),
        })
    });
    let info = MeetingInfo {
        id: Some(id),
        title,
        started_at: Some(jiff::Timestamp::now().to_string()),
        language: None,
    };
    write_json_atomic(&meeting.info_json(), &info)?;
    Ok((meeting, info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("1280x720"), Ok((1280, 720)));
        assert_eq!(parse_size("640X360"), Ok((640, 360)));
        assert!(parse_size("1280").is_err());
        assert!(parse_size("8x8").is_err());
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(Duration::from_secs(3725)), "01:02:05");
    }
}
