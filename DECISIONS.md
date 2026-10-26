# Decisions

One entry per notable decision: what was decided, why, and what else was
considered. Newest entries go at the bottom.

## D1. Rust 1.99.1 pinned, latest crate versions

- **Decision:** `rust-toolchain.toml` pins Rust 1.99.1 (edition 2024). Every
  dependency starts at its latest published version.
- **Why:** reproducible builds across machines and CI. The project owner asked
  for the latest packages.
- **Alternatives:** float on `stable` (CI and local drift apart); an MSRV
  policy (no users to protect yet).

## D2. Crates are created in the milestone that needs them

- **Decision:** M0 creates `sakshi-core`, `sakshi-models`, `sakshi-asr`,
  `sakshi-media`, `sakshi-pipeline`, `apps/sakshi-cli`, and `xtask`.
  `sakshi-store` and `sakshi-mcp` arrive in M2, `sakshi-capture` in M3, and
  `apps/sakshi-desktop` (Tauri 2.x, not the 3.0 alpha) in M4.
- **Why:** empty crates are noise, and creating the desktop app now would
  pull Tauri and Node into CI before anything uses them.
- **Alternatives:** scaffold the full layout from the spec up front.

## D3. Nemotron 3.5 ASR is the only transcription engine

- **Decision:** transcription runs NVIDIA Nemotron 3.5 ASR
  (`nvidia/nemotron-3.5-asr-streaming-0.6b`) through the NeMo-Speech.cpp
  `nemo-speech` sidecar. `sakshi-asr` keeps a `Transcriber` trait so the
  pipeline can be tested with a fake, but it has exactly one real backend.
  There is no Whisper code and no Whisper cargo feature.
- **Why:** project owner's call (spec change, 2026-10-26). One engine keeps
  the build simple; the main binary links no C or C++ at all.
- **Alternatives:** whisper.cpp through `whisper-rs` (the original plan;
  needs cmake and libclang, and links its own ggml); both behind a trait.

## D4. Diarization happens inside the transcribe call; no `sakshi-diarize`

- **Decision:** the `system` track is transcribed and diarized by one
  `nemo-speech transcribe --diar-model …` call that tags every word with a
  speaker. The `sakshi-diarize` crate from the original layout is dropped.
- **Why:** the runtime attributes speakers per word itself, so a separate
  diarization pass and overlap merge would duplicate work.
- **Alternatives:** keep a `Diarizer` trait running `nemo-speech diarize`
  and merging by time overlap. That comes back if the pyannote fallback for
  meetings with more than 8 speakers is built.

## D5. NeMo-Speech.cpp is a pinned sidecar, v0.2.0

- **Decision:** bundle the prebuilt `nemo-speech` v0.2.0 release (binary plus
  its `lib/` folder of ggml and llama dylibs) as a separate process. Model
  downloads are pinned to the Hugging Face revisions and SHA-256 values in
  that release's `models/index.json`.
- **Why:** the runtime is v0.x and its model files change: `main` of the
  ASR repo already holds a newer GGUF (742,090,464 bytes) than the one v0.2.0
  was released against (741,548,352 bytes). Pinning both together keeps them
  compatible. A separate process also keeps its ggml out of our address space.
- **Alternatives:** the `nightly` release tag (moves daily); FFI to
  `libnemo_speech_asr_c` (ties us to a v0.x C ABI).

## D6. Pipeline state lives in the meeting folder during M1

- **Decision:** each meeting folder holds `pipeline.json` recording finished
  stages, and intermediate results go under `work/`. SQLite arrives in M2.
- **Why:** `sakshi-cli process <folder>` must work on any folder without a
  database, and resuming needs no schema yet.
- **Alternatives:** build `sakshi-store` first and keep state in SQLite.

## D7. Sakshi is MIT licensed

- **Decision:** the code is licensed MIT, with a single `LICENSE` file.
- **Why:** project owner's choice: one short, permissive license. Model
  weights keep their own licenses and are shown before download.
- **Alternatives:** `MIT OR Apache-2.0` dual licensing (the Rust ecosystem
  convention, which adds Apache's explicit patent grant); Apache-2.0 only.

## D8. hf-hub 1.0 for downloads, plus one direct API call

- **Decision:** model downloads use `hf-hub` 1.0 (blocking client, `local_dir`
  downloads into a staging folder, then size and SHA-256 checks before the file
  is moved into `models/<owner>/<name>/<revision>/`). Custom models are
  validated before download: `.gguf` only, size and SHA-256 from the Hub's
  `paths-info` API, and `general.architecture` (`asr` or `sortformer`) read
  from the header with HTTP range requests.
- **Why:** the spec's choice and the official client. The project owner
  accepted its cost: about 230 crates including `aws-lc-sys` (C, built with
  `cc`; no cmake needed) and the Xet client. `paths-info` is called with
  reqwest directly because hf-hub 1.0.0 can't report a file's SHA-256: its
  LFS type reads `sha256` while the API sends `oid`, and its HEAD-based
  metadata call fails on Xet-backed files after the CDN redirect.
- **Alternatives:** a small downloader on `ureq`; hf-hub 0.4.

## D9. nemo-speech command lines (confirmed against v0.2.0)

- **Decision:** both tracks run through `nemo-speech transcribe` in its
  cache-aware streaming mode with the largest right context (13 frames,
  1.12 s chunks). Model paths are always absolute local files, so the CLI never
  touches its own model cache or the network.

  `system` track, transcribe and diarize in one pass:

  ```
  nemo-speech transcribe <work>/system.wav \
    --model <models>/…/nemotron-3.5-asr-streaming-0.6b.q8_0.gguf \
    --diar-model <models>/…/Nemotron-3-Diarization.q8_0.gguf --diar-preset v3-offline \
    --stream --asr.streaming.rnnt_right_context 13 --endpointing \
    --language <auto|code> --device auto \
    --format json --output <work>/system.engine.json --force
  ```

  `mic` track: the same without `--diar-model` and `--diar-preset`.

  stderr confirms the geometry:
  `[asr] mode=streaming head=rnnt left=56 center=1 right=13 attention=70 encoder-frames step=1120ms`.
  Sakshi logs this line and fails the run if it reports a different right
  context.

  JSON output (excerpt of `crates/sakshi-asr/tests/data/ami_system.json`):

  ```json
  {
    "file": "system.wav",
    "text": "From a plain text file back into organizer by who which speaker …",
    "confidence": 1,
    "duration": 60,
    "languages": [],
    "words": [
      {"word": "From", "start": 1.28, "end": 1.36, "confidence": 1, "speaker": 1},
      {"word": "a", "start": 1.44, "end": 1.52, "confidence": 1, "speaker": 1},
      …
    ]
  }
  ```

  Times are seconds at 80 ms resolution. `speaker` starts at 1 and is
  omitted when the diarizer assigned no one. `confidence` is always 1 for
  this model in v0.2.0.
- **Why:**
  - **Chunk size:** without `--stream`, files under 6.67 min take the offline
    path, whose attention is fixed by the GGUF at right context 3 (320 ms).
    `rnnt_right_context` only affects the streaming path, and its documented
    `-1 = model max` keeps the GGUF default rather than the maximum. On the
    AMI test clip, right context 13 got "who which speaker", "we're gonna
    do", and "evaluation" right where offline mode did not.
  - **Speed:** on Apple Silicon (Metal) the 60 s clip takes 3.7 s with
    diarization, model load included. The first run after install takes
    about 22 s while Metal compiles its kernels.
  - **Diarizer preset:** `v3-offline` and the default streaming preset gave
    identical speaker turns on the clip. Both matched the AMI reference for
    the three main speakers; one-word backchannels were not transcribed.
- **Alternatives:** the default offline path (less accurate, but reports the
  detected language); a separate `nemo-speech diarize` pass merged by
  overlap (see D4).

## D10. Language: auto by default, pinned codes validated first

- **Decision:** the language is `auto` unless pinned in `config.json`, in a
  meeting's `meeting.json`, or with `--language`. A pinned code must appear
  in the model's own language table (the GGUF key
  `asr.rnnt.prompt_dictionary`, e.g. `hi`, `hi-IN`, `en-US`), or the run fails
  before transcription starts. `transcript.json` lists the pinned code, or
  whatever languages the engine reported.
- **Why:** the runtime looks codes up by exact match and silently falls
  back to auto-detection for anything unknown, so a typo would quietly do
  the wrong thing. In streaming mode v0.2.0 transcribes Hindi correctly under
  `auto`, but reports a language only for utterances that end in sentence
  punctuation (the tag follows it). So a long meeting usually lists its
  language (`en-US` for the 60 min AMI run), while the short fixtures list
  none (`"languages": []`). The offline path reports it reliably but is less
  accurate (D9). Treat the list as best-effort.
- **Open issue (found in M3 live testing):** under `auto`, when the model
  ends a sentence with punctuation and a language tag ("…था।" then
  `<hi-IN>`), it can drop the start of the next utterance. On a recaptured
  copy of the Hindi fixture, 5 s of speech ("इतालवी भाषा में उच्चारण करना
  तुलनात्मक रूप से आसान है") was lost. The same audio with `--language hi-IN`
  was transcribed in full; the original fixture happened not to trigger it.
  Pinning the language avoids it. A possible fix is "detect, then pin": read
  the language from a short offline pass, then transcribe the whole track
  with it pinned. That would also make `languages` reliable, but could hurt
  meetings that switch languages. Not yet decided.
- **Alternatives:** a second, offline pass over a short excerpt just to read
  the detected language (about 3 s extra per track); guessing from the script
  of the text (Devanagari alone doesn't distinguish Hindi from Marathi or
  Nepali).

## D11. Media: ffmpeg for decoding, pure Rust for hashing and JPEG

- **Decision:** audio that is already 16 kHz mono PCM16 or float32 WAV goes
  to the recognizer as is. Anything else is converted by ffmpeg to 16 kHz mono
  float32 under `work/`. Key frames: ffmpeg samples the video once a second
  and downscales to at most 1280 px wide, piping frames as PPM. A 64-bit DCT
  perceptual hash (`image_hasher`) is computed for each. A frame is kept when
  it differs from the last kept frame by at least 10 bits, no sooner than 2 s
  after it, and at least every 45 s regardless. Kept frames are written as
  JPEG quality 80 with the `image` crate. All of these values live in
  `KeyframeConfig`.
- **Why:** nemo-speech reads PCM16 and float32 directly, so re-encoding a
  ready file only costs disk. Decoding stays in ffmpeg (any codec), while
  hashing and JPEG encoding are pure Rust. The 2 s minimum keeps slide
  transitions and animations from producing bursts of frames, and a
  suppressed change is still caught at the next sample.
- **Alternatives:** always re-encoding to float32; ffmpeg's scene-detection
  filter (scores consecutive frames, not frames against the last kept one);
  writing JPEGs from ffmpeg (would need a second pass to know which to keep).
  Development uses the system ffmpeg; release builds will bundle an LGPL
  build (M4).

## D12. Mic-bleed removal works on segments and on runs inside them

- **Decision:** a mic segment is compared, as normalized tokens, with the
  system words within ±2 s of it, using a longest common subsequence.
  - If at least 70% of its tokens match in order, the whole segment is
    dropped (the spec's rule).
  - Otherwise, any run of 4 or more consecutive matched tokens (one
    unmatched token allowed between matches) is cut out, and the rest of
    the segment is kept as the user's speech.
  - Segments under 2 tokens are never touched.
  - Removed pieces go to `work/bleed-removed.json` with their similarity.
  - All four values live in `BleedConfig`; the CLI exposes the threshold.
- **Why:** on the AMI fixture, the user started talking less than 1 s
  after an echo ended, so the echo and the reply formed one segment that
  scored 0.68 and stayed duplicated. Cutting matching runs fixed both such
  cases without touching the user's own words.
- **Alternatives:** segment-level only (misses mixed segments); dropping
  every matched word (a user repeating a word the remote just said would
  lose it); acoustic echo cancellation during capture (M3+, complementary).

## D13. `--endpointing` is required for long meetings

- **Decision:** both transcribe calls pass `--endpointing`, so the runtime
  finalizes each utterance at a pause (800 ms by default) instead of
  returning the whole file as one result.
- **Why:** measured on 60 min of AMI EN2002d (four speakers) on Apple Silicon
  with Metal:

  | `system` command | Speakers tagged | Wall time | Peak RSS |
  |---|---|---|---|
  | without `--endpointing` | one label for every word in the first 48 min | 170 s | 1.5 GB |
  | with `--endpointing` | four speakers throughout; 93% of words match standalone `nemo-speech diarize` | 193 s | 1.5 GB |

  Without endpointing, words are tagged only when the single final result
  is built at the end of the file. By then the diarizer apparently no
  longer holds the frames for most of the recording, so they all fall back
  to one label. Clips of 5 and 15 min were unaffected. With endpointing,
  each utterance is tagged while its frames are still in memory. The
  transcript itself was unaffected: 8,778 words against 8,769, and
  identical text on the 60 s fixture. Standalone diarization of the same
  hour took 21 s.
- **Alternatives:** separate `nemo-speech diarize` plus our own
  word-to-speaker overlap merge (works too, and remains the fallback if a
  later runtime regresses); cutting audio into chunks (speaker numbers
  would not carry across chunks).

## D14. The database indexes meeting folders; the folders stay the truth

- **Decision:**
  - `sakshi.db` (SQLite in WAL mode, bundled SQLite 3.54 with FTS5) indexes
    processed meeting folders wherever they are, by absolute path. It holds
    meeting metadata, segments with their words, frames, and summaries.
  - A meeting gets a UUIDv7 id the first time it is indexed, written to its
    `meeting.json`, so a moved folder keeps its id.
  - Summaries live in `summaries/<id>.md` with a small JSON front matter
    block, so the database can be rebuilt from the folders, summaries
    included.
  - Search uses an FTS5 trigram index. Queries under three characters fall
    back to a LIKE scan.
- **Why:** recordings are large; indexing in place avoids copying them, and
  a lost or corrupt database costs nothing. Word tokenizers split
  Devanagari words at vowel signs, while trigrams match substrings in any
  script ("उच्चार" finds "उच्चारण"). Bundling SQLite gives every
  platform the same version and FTS5; Windows ships no system SQLite.
- **Alternatives:** copying meetings into `<data>/meetings/<id>/` on import
  (planned for recordings Sakshi captures itself, M3); a `unicode61`
  tokenizer; storing summaries only in the database.

## D15. The MCP server writes nothing but summaries

- **Decision:** the server opens the database twice: `Store::open_read_only`
  (read-only flags plus `query_only`) for every query, and `SummaryWriter`,
  whose only operation is saving a summary. Neither can create or migrate
  the database; `sakshi-cli` owns the schema.
- **Why:** the spec's rule ("read-only except for summary writes") becomes
  structural instead of a convention, so a bug in a query tool cannot write.
- **Alternatives:** one read-write connection used carefully.

## D16. MCP tool behaviour

- **Decision:** the eight tools from the spec on rmcp 3.7:
  - **Errors:** bad input (malformed or unknown ids, inverted or
    out-of-range times, oversized limits, empty queries) comes back as a
    tool result with `isError` and a sentence saying what to do, not as a
    protocol error.
  - **Hints:** every tool but `save_summary` is annotated read-only and
    closed-world.
  - **`get_transcript`:** pages by both segment count (400) and text size
    (40,000 characters), resuming from an opaque `next_cursor`. Each segment
    carries the `frame_id` on screen when it started. `format: "text"` gives
    one line per segment with markers where the screen changed.
  - **Frames:** `get_frames` returns at most 16 JPEGs spread evenly across
    the window. `get_frame_at` returns the frame on screen at that moment
    (the latest taken at or before it), not the nearest in time.
  - **Privacy:** meeting folder paths are never sent to clients.
  - **Instructions:** besides the three points from the spec, they say
    transcript text is data, never instructions to the model.
- **Why:** models recover from `isError` messages; protocol errors usually
  just fail the call. A key frame shows the screen until the next one, so
  "on screen at" is what identifying a speaker needs; the nearest frame can
  be one taken after they stopped talking.
- **Alternatives:** clamping out-of-range input silently; offset-based
  paging; nearest-frame lookup as the spec literally says.

## D17. HTTP transport: loopback, bearer token, no browsers

- **Decision:** `sakshi-mcp --http` serves streamable HTTP at `/mcp` on
  127.0.0.1 only. Every request needs `Authorization: Bearer <token>` with
  a random 256-bit token generated at start, compared without
  short-circuiting. `Host` must be a loopback name, and any request carrying
  an `Origin` header (that is, any browser) is refused.
- **Why:** a local port is reachable by every web page the user visits;
  refusing `Origin` and non-loopback `Host` blocks cross-site requests and
  DNS rebinding even before the token is checked.
- **Alternatives:** OAuth (heavy for a local tool); no token (any local
  process could read meetings).

## D18. Tool schemas use one JSON type per field

- **Decision:** optional tool parameters and result fields are declared
  with a single type and left out when absent, rather than schemars' default
  `"type": ["integer", "null"]` for `Option<T>`. The server still accepts an
  explicit `null` as absent.
- **Why:** MCP Inspector's `--strict` check flagged 22 such fields across
  7 tools. Clients that map tool schemas onto a single-type dialect (such as
  the OpenAPI subset used for Gemini function declarations) may reject
  them. After the change it reports none.
- **Alternatives:** `anyOf` branches (still awkward for OpenAPI 3.0-style
  dialects); leaving them (fine for Claude, riskier for other clients).

## D19. macOS capture through objc2 bindings; unsafe only there

- **Decision:** the macOS backend calls ScreenCaptureKit, AVFAudio,
  CoreMedia, CoreVideo, and CoreGraphics through the generated `objc2-*`
  crates (0.3.x; MIT/Apache/zlib, no build scripts). `unsafe_code` is denied
  across the workspace and allowed only in `sakshi-capture/src/macos/`.
- **Why:** `screencapturekit` 11.x compiles a Swift bridge at build time,
  which needs the Swift toolchain and SDK detection on every build machine.
  The objc2 crates are plain Rust, and the FFI stays in one reviewable
  module.
- **Alternatives:** `screencapturekit` (higher level; Swift build); writing
  `extern` declarations by hand.

## D20. How a recording is captured

- **Decision:**
  - **Clock:** one session clock, the host clock (`mach_absolute_time` in
    seconds). ScreenCaptureKit stamps buffers with it, and the microphone's
    `AVAudioTime.hostTime` converts to it.
  - **Audio:** each track is written at its own rate (48 kHz mono for app
    audio, the device rate for the mic). Each buffer is placed at its time:
    late starts and dropouts become silence and overlap is dropped, so
    sample *n* is *n / rate* seconds into the session.
  - **Video:** a ticker writes exactly one frame per tick (default 15 fps,
    the newest frame, or black before the first one) to ffmpeg
    (`h264_videotoolbox`) as raw BGRA. Video is therefore constant-rate and
    starts at the session start, like the audio. ScreenCaptureKit scales
    the window to fit 1280x720.
  - **Crash safety:** everything goes to `<meeting>/capture/`. Video is
    fragmented MP4, and the WAV headers are refreshed every second. A
    `capture.json` is written before the first frame.
  - **Finalize:** produces `recording.mp4` (video copied, both tracks mixed
    to AAC) and 16 kHz mono `mic.wav`/`system.wav`, then deletes the raw
    files. `process` finalizes a capture left behind by a crash.
  - **Audio sources:** a window filter captures that window's app audio and
    excludes Sakshi's own. The microphone comes from AVAudioEngine, which
    works on macOS 14.
  - **Location:** recordings go in `<data>/meetings/<uuid>/`.
- **Why:** ScreenCaptureKit sends frames only when the screen changes, so
  without the ticker the video's timeline would drift from the audio's. The
  spec targets macOS 14+, and ScreenCaptureKit's own microphone capture
  needs macOS 15.
- **Alternatives:** ScreenCaptureKit microphone capture (simpler, 15+);
  AVAssetWriter or `SCRecordingOutput` instead of ffmpeg (the spec fixes
  ffmpeg); variable-frame-rate video with per-frame timestamps; segment
  files instead of fragmented MP4.

## D21. Recording permissions and the consent reminder

- **Decision:** `record` checks Screen Recording (and Microphone, unless
  `--no-mic`) before doing anything. On a first run it triggers the system
  prompt; otherwise it explains exactly which System Settings page to open
  and how. Before each recording it reminds the user to tell participants;
  answering "always", or `config consent-reminder off`, turns that off. The
  setting is stored in `config.json`. Non-interactive runs must pass `--yes`.
- **Why:** for a command-line tool macOS grants access to the app that
  launched it (Terminal, iTerm, ...), which is not obvious, so the messages
  name it. The reminder is the spec's privacy requirement; `--yes` keeps
  scripted use possible without silently skipping it.
- **Alternatives:** a reminder that can't be turned off; asking for
  permissions only when a capture fails.

## D22. Live capture results (macOS 27, Apple Silicon, 2026-10-26)

- **Decision:** M3 ships after these live checks, run with the user's
  consent:

  | Check | Result |
  |---|---|
  | Window capture, 5 s, 15 fps | 77 frames in 5.02 s; H.264 1280x698; the frame shows the window |
  | App audio (display capture while `afplay` plays the Hindi fixture) | Captured audio matches the source within 0.5 dB per second, with no gaps; transcribed in Hindi with two speakers |
  | Mic and system alignment (speakers playing into the mic) | Mic lags system by 60 ms (envelope correlation 0.92), the physical speaker-to-mic delay; 2 echo segments removed by bleed removal |
  | Crash (SIGKILL after 6 s) | Video (6.2 s) and WAV (6.0 s) readable; `process` finalized and processed it |

- **Why:** the backend calls Apple frameworks that unit tests can't
  exercise, and CI runners have no Screen Recording access.
- **Found and fixed:**
  - A command-line process must open its window-server connection
    (`CGMainDisplayID()`) before ScreenCaptureKit, or it aborts with
    `CGS_REQUIRE_INIT`.
  - Bleed trimming could leave mic segments out of time order; they are now
    re-sorted.
- **Not explained:** one scripted run exited with code 1 before creating its
  meeting folder, with stderr discarded. Five identical reruns succeeded.

## D23. One implementation behind the CLI and the desktop app

- **Decision:** a `sakshi-app` crate holds what both front ends do:
  processing a meeting end to end (finish an interrupted capture, run the
  pipeline, index), creating a recording's folder, settings and language
  resolution, and MCP client setup. The pipeline reports progress through
  an observer (`Pipeline::with_observer`, one `StageEvent` per stage
  transition); the CLI prints it and the app forwards it to the window.
- **Why:** the CLI had grown this logic inline. Two copies would drift,
  and the app needs progress the CLI never exposed.
- **Alternatives:** the app shelling out to `sakshi-cli` and parsing its
  output; polling `pipeline.json` for progress.

## D24. Desktop app structure

- **Decision:** Tauri 2 with a React UI. The Rust side is a `Backend` with
  plain methods; `main.rs` only wraps them as commands, run on blocking
  threads because they touch the disk, SQLite, and the capture APIs.
  - Recording runs on its own thread, which owns the platform recording
    (it can't move between threads) and is controlled through a channel.
    When capture ends, whether stopped or interrupted, the meeting goes to
    the processing queue.
  - Processing runs one meeting at a time on a worker thread, so a long
    transcription never blocks recording the next call. A meeting already
    queued isn't queued twice.
  - The backend talks to the window through one event, `sakshi`, whose
    `kind` is `recorder`, `job`, `download`, or `meetings_changed`.
  - Key frames and recordings are shown straight from the meeting folders
    through Tauri's asset protocol. Its scope is the meetings folder, plus
    a meeting folder elsewhere when that meeting is opened.
  - The window gets only `core:default` permissions and a CSP that allows
    the asset protocol for images and media. Opening links and Finder goes
    through backend commands; `open_link` accepts `https://` only.
  - Tests use a fake capture backend and a fake processor. An ignored live
    test drives the real recorder and pipeline.
- **Why:** keeping the backend free of Tauri types lets the record, stop,
  process, and index flow be tested without a window, and keeps the UI a
  thin view of state the backend owns.
- **Alternatives:** one thread per command with the recording in a global;
  running the pipeline inside the recording thread (blocks the next
  recording); granting the window the opener plugin's permissions.

## D25. MCP setup helper: writes JSON configs, prints the rest

- **Decision:** `sakshi-cli mcp setup` and the app's Connect page show
  each client's status and the exact snippet. For Claude Desktop and
  Cursor, whose configs are plain JSON files, Sakshi can add itself:
  - it refuses a file that isn't valid JSON;
  - it keeps every other key, server, and the key order, and keeps extra
    fields on an existing `sakshi` entry (such as `env`);
  - it backs up the old file as `<name>.sakshi-backup-<time>`;
  - it writes through a symlinked config and keeps the file's permissions.

  For Claude Code it shows `claude mcp add --scope user sakshi -- <path>`.
  For Codex it shows the `[mcp_servers.sakshi]` TOML table. `--data-dir`
  is passed only when the data folder isn't the default, because clients
  don't pass on `SAKSHI_DATA_DIR`.
- **Why:** `~/.claude.json` belongs to Claude Code, which rewrites it
  while running, and its own command is the supported way in. Editing
  Codex's TOML safely needs a format-preserving TOML editor, a dependency
  this didn't justify. A wrong edit to a client's config breaks that
  client, hence the backup and the refusals.
- **Alternatives:** writing every client's config; only printing snippets.

## D26. The window list includes windows that aren't on screen

- **Decision:** the source list asks ScreenCaptureKit for all windows, not
  only on-screen ones, and marks each with `on_screen`. A window that isn't
  on screen is listed only if it has a title, is at least 400x300, and
  isn't a system process or an XPC helper (such as the Save panel). The UI
  shows such windows in their own group. A Zoom window counts as a meeting
  only when its title mentions a meeting or webinar.
- **Why:** with on-screen windows only, a call on another desktop, a
  full-screen call (which gets its own desktop), or a minimized one could
  not be picked. Found by the user in the first desktop run. Zoom keeps
  its home window ("Zoom Workplace") and others open between calls, and
  those were all being flagged as meetings.
- **Open:** what a window records while it is minimized or on another
  desktop hasn't been checked live yet. macOS may deliver no new frames
  until the window is visible again; audio is unaffected.
- **Alternatives:** on-screen windows only, with a hint to bring the call
  forward.

## D27. Desktop UI toolchain and conventions

- **Decision:** React 19, Vite 8, and TypeScript 7 with no UI library.
  pnpm 12 installs packages; a committed `.npmrc` points at the public
  npm registry, and CI pins the pnpm version. The window's location is
  kept in the URL hash (`#/meetings/<id>`), so a reload returns to the
  same page.
- **Colours are swappable palettes.** `styles.css` holds layout and type
  only and reads every colour from a token. The tokens live in `src/theme/`:
  - `base.css` holds colours shared by every palette (status, recording,
    speakers);
  - each file in `palettes/` sets about 16 neutral and accent tokens for
    light and dark;
  - `index.ts` lists the palettes and the default, and sets
    `data-palette` and `data-scheme` on `<html>`.

  Settings > Appearance switches palette and System/Light/Dark live, and
  remembers the choice in the window's local storage. Three palettes
  ship: Graphite & Amber (the default, matching the icon), Paper & Ink,
  and Stone & Sage. Adding one is a new CSS file and one line in
  `index.ts`. Accent colours used as text have their own token
  (`--accent-text`), so they stay readable on light backgrounds.
- **Why:** the UI is five pages of forms, lists, and a video player, which
  needs no component library. The first palette's navy sidebar and
  saturated amber felt heavy. The user asked for something calmer, and
  for palettes that can change without touching the rest of the code.
  The `.npmrc` keeps a machine's private registry settings out of the
  build. pnpm 12 couldn't record its own
  version pin in the lockfile here, which made `--frozen-lockfile` fail,
  so the `packageManager` field was dropped.
- **Alternatives:** a component library; npm; a router library.

## D28. Desktop app verification (macOS 27, Apple Silicon, 2026-10-26)

- **Decision:** M4 ships after these checks:
  - Every page was opened in the real app (`pnpm tauri dev`) on a scratch
    data folder and screenshotted, capturing the Sakshi window only:
    - Record: window list from ScreenCaptureKit, setup check.
    - Meetings: list with statuses, transcript, video over the asset
      protocol.
    - Models: both default models detected.
    - Connect: client status and snippets.
    - Settings: language list read from the model, tool checks.
  - The live backend test recorded the app's own window for 5 s on the
    recording thread. The queue then finalized, ran all seven stages, and
    indexed it: 5.7 s, 1 key frame, 0 segments (silent window, mic off).
  - `pnpm tauri build` produced an app bundle.
- **Found and fixed:**
  - Windows on other desktops or minimized were missing (D26).
  - The window scrolled as a whole and cut off the sidebar.
  - Long paths widened pages past the window.
  - The models table pushed its status badges outside the card.
  - At the minimum window size (860x560), the meeting header crushed the
    title.
  - Spacing was too tight in several places.

  Each page was rechecked at the default and the minimum window size.
- **Not checked by hand:** clicking through the consent dialog, a model
  download, and "Add Sakshi to Claude Desktop" in the window. No UI
  automation is available here; the backend paths behind them have unit
  tests, and downloads were verified from the CLI in M1.
