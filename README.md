# Sakshi

Sakshi (साक्षी, "the witness") records video calls in Zoom, Google Meet,
Microsoft Teams, or any window, transcribes them on your machine after the
call, and gives AI clients (Claude, ChatGPT/Codex, Cursor) the transcript and
key screenshots over MCP.

Sakshi records what happened; it doesn't interpret it. It records,
transcribes, separates speakers, and extracts key frames. It never summarizes
and never calls an LLM. Summaries are written by the MCP client and saved back
through Sakshi's `save_summary` tool.

- **Fully local.** No telemetry. The only network access is downloading
  models from Hugging Face when you ask for it.
- **Native inference.** Speech recognition (NVIDIA Nemotron 3.5 ASR) and
  speaker diarization (NVIDIA Nemotron 3 Diarization) run in NVIDIA's
  NeMo-Speech.cpp runtime as a sidecar process. No Python.

## Status

| Milestone | Scope | State |
|---|---|---|
| M0 | Workspace, CI, docs | done |
| M1 | `sakshi-cli process <folder>` on existing recordings | done |
| M2 | SQLite store and MCP server | next |
| M3 | macOS capture | planned |
| M4 | Desktop app (Tauri 2) | planned |
| M5 / M6 | Windows / Linux capture | planned |

Design decisions and measurements are in [DECISIONS.md](DECISIONS.md).

## Quick start (development checkout)

You need the Rust toolchain (rustup installs the version pinned in
`rust-toolchain.toml`) and `ffmpeg` on `PATH`.

```bash
# 1. The pinned NeMo-Speech.cpp runtime (checksum-verified) into target/sidecars/
cargo xtask fetch-sidecar

# 2. The default models (about 850 MB). Each model's license is shown first.
cargo run -p sakshi-cli -- models pull

# 3. Process a meeting folder
cargo run --release -p sakshi-cli -- process path/to/meeting
```

Models and settings live in the data directory: `Sakshi/` under your
platform's application data folder, or `$SAKSHI_DATA_DIR`, or `--data-dir`.

### A meeting folder

| Input | |
|---|---|
| `system.wav` | Meeting or system audio: everyone else |
| `mic.wav` | Your microphone: always labelled "You" |
| `recording.mp4` | Optional video, for key frames |
| `meeting.json` | Optional, e.g. `{"language": "hi"}` to pin this meeting's language |

At least one of the two audio tracks is required. Any format ffmpeg reads
works; 16 kHz mono WAV is used as is.

| Output | |
|---|---|
| `transcript.json` | Segments with `id`, `start_ms`, `end_ms`, `track`, `speaker` ("You" or "Speaker N"), `text`, and per-word timing |
| `frames/<ms>.jpg`, `frames.json` | Key frames (at most 1280 px wide) with their timestamps |
| `pipeline.json` | Finished stages and the settings each ran with |
| `work/` | Intermediate results, including the engine's raw output and the mic segments removed as bleed |

Processing is resumable. Rerunning skips finished stages and redoes only
what changed settings affect. `--from <stage>` or `--force` reruns stages
on request.

### Commands

```text
sakshi-cli process <folder> [--language auto|<code>] [--from <stage>] [--force]
                            [--device auto|cpu|metal|cuda|vulkan]
                            [--asr-model <id>] [--diar-model <id>]
                            [--bleed-threshold <0..1>]
sakshi-cli models list
sakshi-cli models pull [<id>...] [--yes]
sakshi-cli models add <owner/repo> <file.gguf> [--revision <rev>] [--yes]
sakshi-cli config show
sakshi-cli config language <auto|code> [--meeting <folder>]
```

The language defaults to automatic detection. A code you pin is checked
against the model's own language table, for example `hi`, `hi-IN`, or
`en-US`. Nemotron 3.5 ASR covers about 40 locales.

## Models

| Id | Model | Size | License |
|---|---|---|---|
| `nemotron-3.5-asr` | [nvidia/nemotron-3.5-asr-streaming-0.6b](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) (Q8_0 GGUF) | 742 MB | [OpenMDW-1.1](https://openmdw.ai/license/1-1/) |
| `nemotron-3-diarization` | [nvidia/Nemotron-3-Diarization](https://huggingface.co/nvidia/Nemotron-3-Diarization) (Q8_0 GGUF) | 107 MB | [OpenMDW-1.1](https://openmdw.ai/license/1-1/) |

Both are NVIDIA's official GGUF files, pinned to the Hugging Face revisions
that NeMo-Speech.cpp v0.2.0 was released against and verified by SHA-256.
`models add` accepts other GGUF files the runtime can run (Nemotron ASR or
Sortformer diarization); anything else is rejected before download.

## Tests

```bash
cargo test                                  # unit and integration tests (need ffmpeg)
cargo test -p sakshi-cli -- --ignored       # end to end with the real sidecar and models
cargo test -p sakshi-models -- --ignored    # Hugging Face network checks
cargo xtask fixtures                        # regenerate fixtures/ from pinned sources
```

The fixtures are a 60 s English meeting from the AMI corpus and a two-speaker
Hindi clip from FLEURS, both CC BY 4.0. See
[fixtures/README.md](fixtures/README.md).

## Known limitations

- Diarization separates at most 8 remote speakers.
- When the language is auto-detected, the runtime reports it only for some
  utterances, so `transcript.json` may list no language.
- Word confidence is always 1.0 with this model.
- Short mic segments ("yeah", "okay") are never treated as bleed, so a
  one-word echo of the remote side can remain.

## Planned

- SQLite store and MCP server (M2), macOS capture (M3), desktop app (M4),
  then Windows and Linux capture.
- **Converting NeMo checkpoints to GGUF**, so `.nemo` fine-tunes from Hugging
  Face can be added as custom models. Today only ready-made GGUF files are
  accepted.
- A pyannote fallback for meetings with more than 8 speakers.
- Signed installers and `.mcpb` packaging for Claude Desktop.

## License

Sakshi's code is licensed under the [MIT License](LICENSE). Models are
downloaded separately and keep their own licenses, which Sakshi shows before
each download.
