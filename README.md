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

## Install the app (macOS)

Download `Sakshi_<version>_aarch64.dmg` from the
[latest release](https://github.com/navneetlal/sakshi/releases/latest).

### Before you install

- **A Mac with Apple Silicon** (M1 or later) on **macOS 14 Sonoma or
  later**.
- **FFmpeg 8.0 or newer.** Sakshi doesn't include it. With
  [Homebrew](https://brew.sh): `brew install ffmpeg`.
- **About 1 GB of free disk** for the speech models, which the app
  downloads when you ask.
- An internet connection for that download only. Recording, transcription,
  and everything else run offline on your Mac.

### Install

1. Optionally, check the download against the SHA-256 listed in the
   release: `shasum -a 256 ~/Downloads/Sakshi_*_aarch64.dmg`.
2. Open the DMG and drag **Sakshi** onto the **Applications** shortcut
   beside it, then eject the disk image.
3. Sakshi isn't notarized by Apple yet, so macOS blocks it the first time.
   Clear the download flag once, in Terminal:

   ```bash
   xattr -dr com.apple.quarantine /Applications/Sakshi.app
   ```

   Or open the app, then choose **Open Anyway** under System Settings >
   Privacy & Security. The Terminal command also clears the speech engine
   and MCP server inside the app, so macOS doesn't ask again for each.
4. Open Sakshi from Applications.

### First run

1. **Models:** download both default models (about 850 MB in all),
   accepting their license.
2. **Settings:** under Processing tools, FFmpeg, the speech engine, and the
   models should all show a check mark. If FFmpeg is missing, install it
   and reopen the page.
3. **Record:** the first time, click **Allow** for **Screen Recording**
   and **Microphone**, and macOS asks for each. Turn Sakshi on under
   System Settings > Privacy & Security, then quit and reopen Sakshi.
4. **Connect** (optional): to let Claude or another AI client read your
   meetings, open Connect and follow the steps for your client. Sakshi can
   add itself to Claude Desktop and Cursor for you.

### Using the app

- **Record:** pick the call's window (likely meetings are listed first,
  including minimized ones and those on other desktops) or a whole
  display, choose the app's audio and your microphone, tell the
  participants, and start. Stop when the call ends. Sakshi then
  transcribes the recording on your Mac and shows each step as it goes.
- **Meetings:** search titles and transcripts. Open a meeting for the
  video, the transcript with speakers (click a time to play from there),
  key frames, and summaries saved by AI clients. **Name speakers** to
  show "Priya" instead of "Speaker 1", here and to AI clients. Process
  again with another language, or process a recording that never was.
  **Import recording** adds one made elsewhere (see below).
- **Models:** download the default models, or add a GGUF model from any
  Hugging Face repository.
- **Connect:** each AI client's status and setup steps.
- **Settings:** the transcription language, appearance (palette and
  light/dark), the reminder to tell participants, the data folder, and
  whether FFmpeg, the speech engine, and the models are found.

**Keep part of the call window in view while recording.** Sakshi records
what the call app draws, and apps stop drawing windows nobody can see:
every app for a minimized window, and browsers and web-based apps (Meet,
the new Teams) also for one that is fully covered or on another desktop.
Any sliver showing is enough. When video stops, Sakshi says so in the app
and in a notification, with a button to bring the window back; the audio,
and so the transcript, is recorded either way. Details under
[Known limitations](#known-limitations).

Sakshi never summarizes. Ask your AI client to summarize a meeting; it
reads the transcript through Sakshi and saves the summary back, where it
shows under the meeting's Summaries.

**Recordings made elsewhere.** A Zoom, Teams, or Fireflies recording, or
any video or audio file, becomes a meeting like Sakshi's own: choose
Import recording on the Meetings page, or ask your AI client to import the
file (it uses `import_recording` with the file's path; the app opens in the
background and processes it). The file is copied into Sakshi's data
folder, so the original can go. Such a file has one mixed audio track, so
everyone, you included, is "Speaker N"; name them to see who is who.

**Speaker names.** Sakshi never guesses who a speaker is. Name them on the
meeting page with Name speakers, or tell your AI client ("Speaker 1 is
Priya") and it saves the name. Transcripts then show the name, in the app
and to every AI client; a speaker you haven't named stays "Speaker N".
Names live in the meeting's `meeting.json`, so they survive processing the
meeting again.

**Updating:** download the new release and replace the app in
Applications, then clear the download flag again. Screen Recording and
Microphone access carry over. From 0.2.1 or earlier, which were signed
differently, the old permissions look on but don't apply: once, remove
Sakshi from both lists in System Settings > Privacy & Security (−), then
click **Allow** on the Record page again. **Uninstalling:** delete
the app. Models, settings, and meetings stay in
`~/Library/Application Support/Sakshi` until you delete that folder.

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

The pages are described under [Using the app](#using-the-app). Appearance
offers the Sakshi brand palette by default and three alternatives.

To change the colours, edit or add a palette in
`apps/sakshi-desktop/src/theme/palettes/` and list it in
`src/theme/index.ts`. The rest of the UI only uses the palette's tokens.
Whatever the palette, recording is shown by the Tally disc (see Brand).

When run with `pnpm tauri dev`, macOS attributes Screen Recording and
Microphone access to your terminal app, as with the CLI.

To build the app as released:

```bash
cd apps/sakshi-desktop
pnpm run bundle         # -> target/release/bundle/macos/Sakshi.app
                        #    and target/release/bundle/dmg/Sakshi_<version>_aarch64.dmg
```

`Sakshi.app` (about 55 MB) contains the speech engine and the MCP server,
so it works on any Apple Silicon Mac with macOS 14+ and FFmpeg installed.
AI clients are set up to launch the `sakshi-mcp` inside it, so if you move
the app, the Connect page shows the clients as set up with a different
command until you set them up again. The app is signed with a self-signed
certificate, **Sakshi Code Signing**, from your login keychain, so macOS
keeps its permissions from one release to the next (DECISIONS D38); it
isn't signed with a Developer ID or notarized, see [Install](#install) for
opening it on another Mac. The DMG itself is left unsigned, because
Gatekeeper refuses to open a signed disk image that isn't notarized.
Without that certificate, build with `APPLE_SIGNING_IDENTITY=- pnpm run
bundle` to sign ad hoc.

To publish a release, attach the DMG and its SHA-256 to a GitHub release
for the tag `v<version>`. The DMG holds the app beside an Applications
shortcut; `CI=true` skips Tauri's Finder scripting for its window layout.

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

If the window's video pauses because the window is out of view, the
command prints when it pauses and resumes, and the totals at the end; the
spans are kept in the meeting's `capture/capture.json` under
`stats.video_pauses`, and indexed, so AI clients are told which key frames
are out of date.

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
| `meeting.json` | Gains the meeting's `id` when it is first indexed; also holds `speaker_names`, and `imported_from` for an import |
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
sakshi-cli import <file> [--title <text>] [--started-at <time>] [--language auto|<code>]
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
`save_summary`, `set_speaker_names`, and `import_recording`, which leaves
the file for the app to process.

| Tool | |
|---|---|
| `list_meetings(since?, until?, query?, limit?)` | Meetings, most recent first |
| `get_meeting(meeting_id)` | Title, times, duration, speakers with any names, frame count, where the video paused, saved summaries; for an import still running, how it's going |
| `get_transcript(meeting_id, start_ms?, end_ms?, format?, include_words?, cursor?)` | Segments with speakers (names where given), times, the key frame on screen, and whether the video was paused, in pages |
| `get_frames(meeting_id, start_ms?, end_ms?, max?)` | Key frames as images (up to 16) |
| `get_frame_at(meeting_id, at_ms)` | The key frame on screen at a moment |
| `search_transcripts(query, limit?)` | Matching segments across meetings |
| `save_summary(meeting_id, markdown, title?, author_model?)` | Stores a summary the client wrote |
| `list_summaries(meeting_id)` | Saved summaries with their Markdown |
| `set_speaker_names(meeting_id, names)` | Names speakers by label, e.g. `{"Speaker 1": "Priya"}`; an empty name removes one |
| `import_recording(path, title?, started_at?, language?)` | Queues a recording made elsewhere for the app; returns the meeting's id at once |

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
- Window video depends on the app drawing the window, and apps stop
  drawing windows that can't be seen. Checked live on macOS 27
  (DECISIONS.md D32):

  | The call window is…            | Native app (AppKit) | Web-based app (WebKit) |
  | ------------------------------ | ------------------- | ---------------------- |
  | in view, even only a sliver    | recorded            | recorded               |
  | fully covered by other windows | recorded            | stops, or ~1 update/s  |
  | on another desktop             | recorded            | stops                  |
  | minimized                      | stops               | stops                  |

  No capture API can record pixels an app hasn't drawn, so Sakshi can't
  prevent this. It watches for it instead: when the window is out of view
  and its video stops, the app shows a warning with a button to bring the
  window back, sends a notification, and keeps the pause in the meeting's
  `capture.json`. The recording holds the last picture through a pause.
  Audio is never affected. Chromium browsers weren't tested; their
  published design throttles hidden windows the same way.
- `Sakshi.app` isn't notarized, so macOS blocks it the first time until
  you clear its download flag (see [Install](#install)).
- An imported recording has one mixed track, so diarization alone tells
  the speakers apart, and it can merge two voices: four voices in the AMI
  fixture came out as three speakers. Naming them, or giving two labels
  the same name, is how to fix it up.
- An imported audio-only file has no player in the app yet, so its
  transcript times can't be played.

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
