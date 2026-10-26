use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{
    AsrError, LanguageTable, NEMO_SPEECH_VERSION, RawTranscript, TranscribeRequest, Transcriber,
    parse_transcribe_json,
};

/// Cache-aware right context in encoder frames: 13 gives 1.12 s chunks, the
/// most accurate setting Nemotron 3.5 ASR supports (D9 in DECISIONS.md).
pub const RIGHT_CONTEXT: u32 = 13;

/// Largest diarizer chunks for Sortformer V3 (Nemotron 3 Diarization).
const DIAR_PRESET: &str = "v3-offline";

/// Lines of sidecar stderr kept for error messages.
const STDERR_TAIL: usize = 40;

/// The `nemo-speech` sidecar running Nemotron ASR, optionally with
/// Sortformer diarization in the same pass.
#[derive(Debug, Clone)]
pub struct NemoSpeech {
    binary: PathBuf,
    asr_model: PathBuf,
    diar_model: Option<PathBuf>,
    languages: LanguageTable,
    device: String,
}

impl NemoSpeech {
    /// Checks the sidecar's version and reads the ASR model's language table.
    /// Model paths must be local files: the sidecar is never given a
    /// repository id, so it never downloads anything itself.
    pub fn new(
        binary: PathBuf,
        asr_model: PathBuf,
        diar_model: Option<PathBuf>,
    ) -> Result<Self, AsrError> {
        check_version(&binary)?;
        let languages = LanguageTable::from_model(&asr_model)?;
        Ok(Self {
            binary,
            asr_model,
            diar_model,
            languages,
            device: "auto".into(),
        })
    }

    /// `auto` (default), `cpu`, `metal`, `cuda[:N]`, or `vulkan[:N]`.
    pub fn with_device(mut self, device: impl Into<String>) -> Self {
        self.device = device.into();
        self
    }

    pub fn languages(&self) -> &LanguageTable {
        &self.languages
    }

    fn args(&self, req: &TranscribeRequest<'_>, language: &str) -> Result<Vec<OsString>, AsrError> {
        let mut args: Vec<OsString> = vec![
            "transcribe".into(),
            req.audio.into(),
            "--model".into(),
            self.asr_model.clone().into(),
        ];
        if req.diarize {
            let diar = self
                .diar_model
                .as_ref()
                .ok_or(AsrError::NoDiarizationModel)?;
            args.extend([
                "--diar-model".into(),
                diar.clone().into(),
                "--diar-preset".into(),
                DIAR_PRESET.into(),
            ]);
        }
        args.extend([
            "--stream".into(),
            "--asr.streaming.rnnt_right_context".into(),
            RIGHT_CONTEXT.to_string().into(),
            // Finalize each utterance at a pause. Without this the whole
            // file is one final whose words are tagged only at the end, when
            // the diarizer no longer holds the early frames: on a 60 min
            // meeting the first 48 min all got one speaker (D13).
            "--endpointing".into(),
            "--language".into(),
            language.into(),
            "--device".into(),
            self.device.clone().into(),
            "--format".into(),
            "json".into(),
            "--output".into(),
            req.raw_output.into(),
            "--force".into(),
        ]);
        Ok(args)
    }
}

impl Transcriber for NemoSpeech {
    fn engine_id(&self) -> String {
        let name = |p: &std::path::Path| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let mut id = format!(
            "nemo-speech {NEMO_SPEECH_VERSION} asr={} right_context={RIGHT_CONTEXT}",
            name(&self.asr_model)
        );
        if let Some(diar) = &self.diar_model {
            id.push_str(&format!(" diar={} preset={DIAR_PRESET}", name(diar)));
        }
        id
    }

    fn transcribe(&self, req: &TranscribeRequest<'_>) -> Result<RawTranscript, AsrError> {
        let language = self.languages.resolve(req.language)?;
        let args = self.args(req, &language)?;
        if let Some(parent) = req.raw_output.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AsrError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        tracing::info!(
            audio = %req.audio.display(),
            diarize = req.diarize,
            language = %language,
            "running nemo-speech"
        );
        let mut child = Command::new(&self.binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| AsrError::Spawn {
                path: self.binary.clone(),
                source,
            })?;

        let mut tail = VecDeque::with_capacity(STDERR_TAIL);
        let mut mode_line = None;
        let mut stderr = BufReader::new(child.stderr.take().expect("stderr is piped"));
        let mut buf = Vec::new();
        while stderr.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
            let line = String::from_utf8_lossy(&buf).trim_end().to_owned();
            buf.clear();
            tracing::debug!(target: "nemo_speech", "{line}");
            if line.starts_with("[asr] mode=") {
                mode_line = Some(line.clone());
            }
            if tail.len() == STDERR_TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        let status = child.wait().map_err(|source| AsrError::Spawn {
            path: self.binary.clone(),
            source,
        })?;
        if !status.success() {
            return Err(AsrError::SidecarFailed {
                status: status.to_string(),
                stderr: Vec::from(tail).join("\n"),
            });
        }
        check_geometry(mode_line.as_deref())?;

        let json = std::fs::read_to_string(req.raw_output).map_err(|source| AsrError::Io {
            path: req.raw_output.to_path_buf(),
            source,
        })?;
        parse_transcribe_json(&json).map_err(|source| AsrError::Output {
            path: req.raw_output.to_path_buf(),
            source,
        })
    }
}

fn check_version(binary: &std::path::Path) -> Result<(), AsrError> {
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|source| AsrError::Spawn {
            path: binary.to_path_buf(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = stdout
        .trim()
        .strip_prefix("nemo-speech ")
        .unwrap_or(stdout.trim());
    if found != NEMO_SPEECH_VERSION {
        return Err(AsrError::SidecarVersion {
            path: binary.to_path_buf(),
            found: found.to_owned(),
            expected: NEMO_SPEECH_VERSION.to_owned(),
        });
    }
    Ok(())
}

/// Fails unless the sidecar reported streaming with [`RIGHT_CONTEXT`].
fn check_geometry(mode_line: Option<&str>) -> Result<(), AsrError> {
    let Some(line) = mode_line else {
        tracing::warn!("nemo-speech did not report its decoding mode; geometry unverified");
        return Ok(());
    };
    let want = format!("right={RIGHT_CONTEXT}");
    if line.contains("mode=streaming") && line.split_whitespace().any(|t| t == want) {
        Ok(())
    } else {
        Err(AsrError::UnexpectedGeometry {
            line: line.to_owned(),
            expected: RIGHT_CONTEXT,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sakshi_core::Language;

    use super::*;

    fn backend(diar: bool) -> NemoSpeech {
        NemoSpeech {
            binary: "nemo-speech".into(),
            asr_model: "/m/asr.gguf".into(),
            diar_model: diar.then(|| "/m/diar.gguf".into()),
            languages: LanguageTable::from_metadata(&sakshi_models::gguf::GgufMetadata {
                version: 3,
                tensor_count: 0,
                kv: Default::default(),
            }),
            device: "auto".into(),
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn system_track_args_transcribe_and_diarize() {
        let req = TranscribeRequest {
            audio: Path::new("/w/system.wav"),
            language: &Language::Auto,
            diarize: true,
            raw_output: Path::new("/w/system.nemo.json"),
        };

        let args = strings(backend(true).args(&req, "auto").unwrap());

        assert_eq!(
            args.join(" "),
            "transcribe /w/system.wav --model /m/asr.gguf \
             --diar-model /m/diar.gguf --diar-preset v3-offline \
             --stream --asr.streaming.rnnt_right_context 13 --endpointing \
             --language auto --device auto \
             --format json --output /w/system.nemo.json --force"
        );
    }

    #[test]
    fn mic_track_args_skip_diarization() {
        let req = TranscribeRequest {
            audio: Path::new("/w/mic.wav"),
            language: &Language::Code("hi".into()),
            diarize: false,
            raw_output: Path::new("/w/mic.nemo.json"),
        };

        let args = strings(backend(true).args(&req, "hi").unwrap());

        assert!(!args.iter().any(|a| a.starts_with("--diar")));
        assert!(args.windows(2).any(|p| p == ["--language", "hi"]));
    }

    #[test]
    fn diarizing_without_a_model_fails() {
        let req = TranscribeRequest {
            audio: Path::new("a.wav"),
            language: &Language::Auto,
            diarize: true,
            raw_output: Path::new("a.json"),
        };
        assert!(matches!(
            backend(false).args(&req, "auto"),
            Err(AsrError::NoDiarizationModel)
        ));
    }

    #[test]
    fn geometry_is_checked_from_the_captured_stderr() {
        let mode = |stderr: &str| {
            stderr
                .lines()
                .find(|l| l.starts_with("[asr] mode="))
                .map(str::to_owned)
        };
        let ok = mode(include_str!("../tests/data/ami_system.stderr.txt"));
        assert!(check_geometry(ok.as_deref()).is_ok());

        let offline =
            "[asr] mode=offline head=rnnt attention-left=56 attention-right=3 encoder-frames";
        assert!(check_geometry(Some(offline)).is_err());
        let small = "[asr] mode=streaming head=rnnt left=56 center=1 right=1 attention=58";
        assert!(check_geometry(Some(small)).is_err());
        assert!(check_geometry(None).is_ok());
    }
}
