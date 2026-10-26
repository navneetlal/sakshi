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
| M2 | SQLite store and MCP server | done |
| M3 | macOS capture | next |
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

# 3. Process a meeting folder (also indexes it for the MCP server)
cargo run --release -p sakshi-cli -- process path/to/meeting

# 4. Build the MCP server that AI clients launch
cargo build --release -p sakshi-mcp-server     # -> target/release/sakshi-mcp
```

Models, settings, and the meeting index (`sakshi.db`) live in the data
directory: `Sakshi/` under your platform's application data folder, or
`$SAKSHI_DATA_DIR`, or `--data-dir`. Meeting folders stay where they are;
the index points at them.

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
| `meeting.json` | Gains the meeting's `id` when it is first indexed |
| `summaries/<id>.md` | Summaries saved by MCP clients |

Processing is resumable. Rerunning skips finished stages and redoes only
what changed settings affect. `--from <stage>` or `--force` reruns stages
on request.

### Commands

```text
sakshi-cli process <folder> [--language auto|<code>] [--from <stage>] [--force]
                            [--device auto|cpu|metal|cuda|vulkan]
                            [--asr-model <id>] [--diar-model <id>]
                            [--bleed-threshold <0..1>]
sakshi-cli meetings list [--query <text>] [--limit <n>]
sakshi-cli models list
sakshi-cli models pull [<id>...] [--yes]
sakshi-cli models add <owner/repo> <file.gguf> [--revision <rev>] [--yes]
sakshi-cli config show
sakshi-cli config language <auto|code> [--meeting <folder>]
```

The language defaults to automatic detection. A code you pin is checked
against the model's own language table, for example `hi`, `hi-IN`, or
`en-US`. Nemotron 3.5 ASR covers about 40 locales.

## Using Sakshi from AI clients (MCP)

`sakshi-mcp` reads the meeting index and folders directly, so it works
whether or not anything else is running. It only reads, except for
`save_summary`.

| Tool | |
|---|---|
| `list_meetings(since?, until?, query?, limit?)` | Meetings, most recent first |
| `get_meeting(meeting_id)` | Title, times, duration, speaker labels, frame count, saved summaries |
| `get_transcript(meeting_id, start_ms?, end_ms?, format?, include_words?, cursor?)` | Segments with speakers, times, and the key frame on screen, in pages |
| `get_frames(meeting_id, start_ms?, end_ms?, max?)` | Key frames as images (up to 16) |
| `get_frame_at(meeting_id, at_ms)` | The key frame on screen at a moment |
| `search_transcripts(query, limit?)` | Matching segments across meetings |
| `save_summary(meeting_id, markdown, title?, author_model?)` | Stores a summary the client wrote |
| `list_summaries(meeting_id)` | Saved summaries with their Markdown |

**Claude Desktop** (`claude_desktop_config.json`) and **Cursor**
(`~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "sakshi": { "command": "/absolute/path/to/sakshi-mcp" }
  }
}
```

**Claude Code:** `claude mcp add sakshi -- /absolute/path/to/sakshi-mcp`

**Codex** (`~/.codex/config.toml`):

```toml
[mcp_servers.sakshi]
command = "/absolute/path/to/sakshi-mcp"
```

Add `"args": ["--data-dir", "/path"]` (or `args = [...]`) if your data
directory isn't the default.

**Streamable HTTP**, for clients that can't launch a process:

```bash
sakshi-mcp --http --port 8790 --token-file ~/.sakshi-mcp-token
```

It listens on `http://127.0.0.1:8790/mcp` only and requires
`Authorization: Bearer <token from the file>` on every request. Browsers
are refused.

To try the server by hand, use the MCP Inspector:
`npx @modelcontextprotocol/inspector /absolute/path/to/sakshi-mcp`.

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
cargo test                                  # unit, integration, and MCP protocol tests (need ffmpeg)
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

- macOS capture (M3), desktop app with an MCP setup helper (M4), then
  Windows and Linux capture.
- **Converting NeMo checkpoints to GGUF**, so `.nemo` fine-tunes from Hugging
  Face can be added as custom models. Today only ready-made GGUF files are
  accepted.
- A pyannote fallback for meetings with more than 8 speakers.
- Signed installers and `.mcpb` packaging for Claude Desktop.

## License

Sakshi's code is licensed under the [MIT License](LICENSE). Models are
downloaded separately and keep their own licenses, which Sakshi shows before
each download.
