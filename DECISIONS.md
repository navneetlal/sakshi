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
