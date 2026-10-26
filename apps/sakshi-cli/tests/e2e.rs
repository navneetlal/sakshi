//! End-to-end runs of `sakshi-cli process` on the fixtures with the real
//! nemo-speech sidecar and models. They need, once per checkout:
//!
//! ```text
//! cargo xtask fetch-sidecar
//! cargo run -p sakshi-cli -- --data-dir target/dev-data models pull --yes
//! ```
//!
//! and ffmpeg on PATH. Run with `cargo test -p sakshi-cli -- --ignored`.
//! `SAKSHI_DATA_DIR` overrides where the models are looked up.

use std::path::{Path, PathBuf};
use std::process::Command;

use sakshi_core::{Frame, Speaker, Track, Transcript, read_json};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn data_dir() -> PathBuf {
    std::env::var_os("SAKSHI_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("target/dev-data"))
}

/// Copies a fixture into a fresh temporary meeting folder.
fn meeting(fixture: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(workspace().join("fixtures").join(fixture)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, dir.path().join(path.file_name().unwrap())).unwrap();
    }
    dir
}

fn process(folder: &Path, extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_sakshi-cli"))
        .arg("--data-dir")
        .arg(data_dir())
        .arg("process")
        .arg(folder)
        .args(extra)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "sakshi-cli failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn speaker_saying(t: &Transcript, phrase: &str) -> Speaker {
    t.segments
        .iter()
        .find(|s| s.text.to_lowercase().contains(phrase))
        .unwrap_or_else(|| panic!("no segment contains {phrase:?}"))
        .speaker
}

#[test]
#[ignore = "needs the nemo-speech sidecar, downloaded models, and ffmpeg"]
fn english_meeting_end_to_end() {
    let dir = meeting("ami-en2002d");

    let stdout = process(dir.path(), &[]);

    let t: Transcript = read_json(&dir.path().join("transcript.json")).unwrap();
    let remote: std::collections::BTreeSet<Speaker> = t
        .segments
        .iter()
        .filter(|s| s.track == Track::System)
        .map(|s| s.speaker)
        .collect();
    assert!(
        (2..=4).contains(&remote.len()),
        "remote speakers: {remote:?}"
    );
    assert!(remote.iter().all(|s| matches!(s, Speaker::Remote(_))));
    // MEE071 opens the excerpt and MEE073 answers near the end.
    assert_eq!(speaker_saying(&t, "plain text file"), Speaker::Remote(1));
    assert_ne!(
        speaker_saying(&t, "probably not"),
        speaker_saying(&t, "plain text file")
    );

    // FEO070 is "You", and the remote speech leaking into the mic is gone.
    assert_eq!(speaker_saying(&t, "thursday"), Speaker::You);
    let mic_text: String = t
        .segments
        .iter()
        .filter(|s| s.track == Track::Mic)
        .map(|s| s.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    for echo in ["plain text file", "organize it by time", "probably not"] {
        assert!(
            !mic_text.contains(echo),
            "mic still has {echo:?}: {mic_text}"
        );
    }

    // Segments are numbered in time order.
    assert!(
        t.segments
            .iter()
            .enumerate()
            .all(|(i, s)| s.id as usize == i)
    );
    assert!(
        t.segments
            .windows(2)
            .all(|p| p[0].start_ms <= p[1].start_ms)
    );

    let frames: Vec<Frame> = read_json(&dir.path().join("frames.json")).unwrap();
    let times: Vec<u64> = frames.iter().map(|f| f.at_ms).collect();
    assert_eq!(times, [0, 20_000, 40_000]);
    assert!(dir.path().join("frames/20000.jpg").is_file());
    assert!(stdout.contains("key frames: 3"), "{stdout}");

    // A second run reuses every stage.
    let again = process(dir.path(), &[]);
    assert!(again.contains("ran:        none"), "{again}");
}

#[test]
#[ignore = "needs the nemo-speech sidecar, downloaded models, and ffmpeg"]
fn hindi_meeting_auto_detects_and_separates_speakers() {
    let dir = meeting("fleurs-hi");

    process(dir.path(), &[]);

    let t: Transcript = read_json(&dir.path().join("transcript.json")).unwrap();
    assert_eq!(t.segments.len(), 2, "{:#?}", t.segments);
    assert_eq!(t.segments[0].speaker, Speaker::Remote(1));
    assert_eq!(t.segments[1].speaker, Speaker::Remote(2));
    assert!(
        t.segments[0].text.contains("मंगलवार"),
        "{}",
        t.segments[0].text
    );
    assert!(
        t.segments[1].text.contains("इतालवी"),
        "{}",
        t.segments[1].text
    );
    // Streaming mode doesn't report the detected language (DECISIONS D10).
    assert!(t.languages.is_empty());

    // Pinning the language redoes transcription and records the pin.
    let stdout = process(dir.path(), &["--language", "hi"]);
    assert!(stdout.contains("transcribe-system"), "{stdout}");
    let t: Transcript = read_json(&dir.path().join("transcript.json")).unwrap();
    assert_eq!(t.languages, ["hi"]);
    assert!(t.segments[0].text.contains("मंगलवार"));
}

#[test]
#[ignore = "needs the nemo-speech sidecar and downloaded models"]
fn unsupported_language_fails_before_transcribing() {
    let dir = meeting("fleurs-hi");
    let out = Command::new(env!("CARGO_BIN_EXE_sakshi-cli"))
        .arg("--data-dir")
        .arg(data_dir())
        .arg("process")
        .arg(dir.path())
        .args(["--language", "xx"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not support language \"xx\""),
        "{stderr}"
    );
    assert!(!dir.path().join("work/system.engine.json").exists());
}
