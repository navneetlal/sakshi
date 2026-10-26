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
    --stream --asr.streaming.rnnt_right_context 13 \
    --language <auto|code> --device auto \
    --format json --output <work>/system.nemo.json --force
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
  `auto`, but reports no detected language (`"languages": []`) and drops the
  final sentence punctuation. The offline path reports both but is less
  accurate (D9), so for now an auto-detected meeting has an empty language
  list.
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
