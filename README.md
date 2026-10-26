<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-horizontal-reverse.svg">
    <img src="assets/brand/logo-horizontal.svg" alt="Sakshi" width="300">
  </picture>
</h1>

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
| M3 | macOS capture (`sakshi-cli record`) | done |
| M4 | Desktop app (Tauri 2) | done |
| M5 / M6 | Windows / Linux capture | planned |

Design decisions and measurements are in [DECISIONS.md](DECISIONS.md).

## Requirements

- **macOS 14 or later on Apple Silicon** for recording and the desktop
  app. Processing recordings with `sakshi-cli` also works on Windows and
  Linux.
- **FFmpeg 8.0 or newer**, installed separately; Sakshi doesn't include it.
  On macOS: `brew install ffmpeg`. Sakshi looks for it on `PATH` and in
  the Homebrew and MacPorts folders (`/opt/homebrew/bin`, `/usr/local/bin`,
  `/opt/local/bin`), or uses the binary named by `SAKSHI_FFMPEG`. Tested
  with FFmpeg 8.0.1 and 9.0.2. Releases back to 4.4 have every feature
  Sakshi uses but aren't tested; the app's Settings page warns about
  anything older than 8.0.
- About 1 GB of disk for the speech models, downloaded on first use.

## Quick start (development checkout)

You need the Rust toolchain (rustup installs the version pinned in
`rust-toolchain.toml`) and FFmpeg (see Requirements).

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

### The desktop app

Needs Node.js 24 and pnpm, plus steps 1 and 4 above (the app uses the
sidecar from `target/sidecars/` and the `sakshi-mcp` built next to it).

```bash
cd apps/sakshi-desktop
pnpm install
pnpm tauri dev          # builds and opens the app; the UI reloads as you edit
```

- **Record:** pick a window or display (likely meetings first, then other
  windows, including minimized ones and those on other desktops), choose
  the app's audio and your microphone, and start. A timer runs while
  recording. Stopping hands the meeting to processing, shown step by step.
- **Meetings:** search titles and transcripts; open a meeting to see the
  video, the transcript with speakers (click a time to play from there),
  key frames, and summaries saved by AI clients. Process again with
  another language, or process a recording that never was.
- **Models:** download the default models after accepting their license,
  with progress; add a GGUF model from any Hugging Face repository.
- **Connect:** each AI client's status and setup snippet. Sakshi can add
  itself to Claude Desktop's and Cursor's config files, backing them up
  first.
- **Settings:** transcription language, appearance (the Sakshi brand
  palette by default, three alternatives, and System/Light/Dark), the
  consent reminder, the data folder, and whether ffmpeg, the speech engine,
  and the models are found.

To change the colours, edit or add a palette in
`apps/sakshi-desktop/src/theme/palettes/` and list it in
`src/theme/index.ts`. The rest of the UI only uses the palette's tokens.
Whatever the palette, recording is shown by the Tally disc (see Brand).

When run with `pnpm tauri dev`, macOS attributes Screen Recording and
Microphone access to your terminal app, as with the CLI.

To build the app for installing:

```bash
cd apps/sakshi-desktop
pnpm run bundle         # -> target/release/bundle/macos/Sakshi.app
```

`Sakshi.app` (about 55 MB) contains the speech engine and the MCP server,
so it works on any Apple Silicon Mac with macOS 14+ and FFmpeg installed.
Copy it to `/Applications`. AI clients are then set up to launch
`/Applications/Sakshi.app/Contents/MacOS/sakshi-mcp`; if you move the app,
the Connect page shows the clients as set up with a different command
until you set them up again. Models and meetings stay in the data folder,
outside the app.

The app is ad-hoc signed but not yet signed with a Developer ID or
notarized. On another Mac, open it the first time from System Settings >
Privacy & Security > Open Anyway (or right-click > Open). Once installed,
the app itself needs Screen Recording and Microphone access.

### Recording a call (macOS 14+)

```bash
sakshi-cli record --list                 # windows and displays; likely meetings marked *
sakshi-cli record                        # pick one interactively
sakshi-cli record --app zoom             # or --window <id> / --display <id>
```

Sakshi records the window's video (scaled to fit 1280x720, 15 fps), that
app's audio (the `system` track: everyone else), and your microphone (the
`mic` track: "You"). Press Ctrl-C to stop. The recording is then
transcribed and indexed, and lands in `<data>/meetings/<id>/`. Useful flags:
`--no-mic`, `--no-system-audio`, `--fps`, `--max-size 1920x1080`,
`--duration <s>`, `--no-process`, `--title`.

Before each recording Sakshi reminds you to tell participants they're being
recorded. Answer `a` to stop the reminder, or run `sakshi-cli config
consent-reminder off`.

macOS asks for **Screen Recording** and **Microphone** access the first
time. For a command-line tool the permission belongs to the app you run it
from (Terminal, iTerm, ...). Turn it on in System Settings > Privacy &
Security, then quit and reopen that app.

If Sakshi is interrupted mid-recording, the capture files stay readable;
`sakshi-cli process <meeting folder>` finishes the recording and processes
it.

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
sakshi-cli config consent-reminder <on|off>
sakshi-cli record [--list] [--window <id> | --display <id> | --app <text>] [...]
sakshi-cli mcp setup [--client <name> [--write]] [--server <path>]
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

The quickest setup is the app's Connect page, or:

```bash
sakshi-cli mcp setup                                   # every client: status and what to add
sakshi-cli mcp setup --client claude-desktop --write   # or cursor: edits the config, after a backup
```

It fills in the path to `sakshi-mcp` and your data folder. By hand:

**Claude Desktop** (`claude_desktop_config.json`) and **Cursor**
(`~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "sakshi": { "command": "/absolute/path/to/sakshi-mcp" }
  }
}
```

**Claude Code:** `claude mcp add --scope user sakshi -- /absolute/path/to/sakshi-mcp`

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
cargo test -p sakshi-desktop -- --ignored   # records the running app's window, then processes it (macOS)
(cd apps/sakshi-desktop && pnpm run build)  # type-check and build the UI
cargo xtask fixtures                        # regenerate fixtures/ from pinned sources
cargo xtask brand-icons                     # check the brand icons against the spec (--write redraws them)
```

The fixtures are a 60 s English meeting from the AMI corpus and a two-speaker
Hindi clip from FLEURS, both CC BY 4.0. See
[fixtures/README.md](fixtures/README.md).

## Known limitations

- Diarization separates at most 8 remote speakers.
- When the language is auto-detected, the runtime reports it only for some
  utterances, so `transcript.json` may list no language.
- Auto-detection can drop the start of an utterance that follows a sentence
  end (seen with Hindi). If you know the meeting's language, pin it with
  `--language` or `sakshi-cli config language` (DECISIONS.md D10).
- On macOS, a window capture records that app's audio. Recording a display
  records all system audio except Sakshi's own.
- Word confidence is always 1.0 with this model.
- Short mic segments ("yeah", "okay") are never treated as bleed, so a
  one-word echo of the remote side can remain.
- A window that is minimized or on another desktop can be picked, but
  macOS may not deliver new video frames for it until it's visible again
  (not yet checked live; DECISIONS.md D26). Audio is unaffected.
- `Sakshi.app` isn't notarized, so macOS asks before opening it the first
  time (see above).

## Planned

- Windows and Linux capture (M5, M6), with their app packages.
- **Converting NeMo checkpoints to GGUF**, so `.nemo` fine-tunes from Hugging
  Face can be added as custom models. Today only ready-made GGUF files are
  accepted.
- A pyannote fallback for meetings with more than 8 speakers.
- Signed installers and `.mcpb` packaging for Claude Desktop.

## Brand

The mark is a filled disc, the recording, resolving into three bars, the
transcript. The disc is in Tally amber: the one warm colour in the identity
and the one thing that signals state, so in the app it marks recording and
nothing else. Interface colour comes from Ink, Paper, Graphite, and Rule.

| | Hex | Use |
|---|---|---|
| Ink | `#12151C` | Bars, wordmark, type, dark surfaces |
| Tally | `#E8921E` | The disc only, never type or behind type |
| Paper | `#F7F7F5` | The page; the mark reversed on Ink |
| Graphite | `#4A5160` | Secondary type |
| Rule | `#E0E1DE` | Hairlines and card edges |

The mark comes in three cuts: wide for 64 px and up, stacked for squares
of 24 px and up (app icons), and micro below 24 px (tray, favicon). The
wordmark is Space Grotesk Medium, shipped as outlines.

The app sets its interface and body in Inter, its headings in Space
Grotesk Medium, and Devanagari text, such as Hindi transcripts, in Noto
Sans Devanagari. All three are bundled with the app under the SIL Open Font
License 1.1, so nothing loads from the network.

Logos, icons, the social card, and the full spec, including its six usage
rules, are in [assets/brand/](assets/brand/README.md).

## License

Sakshi's code is licensed under the [MIT License](LICENSE). Models are
downloaded separately and keep their own licenses, which Sakshi shows before
each download.
