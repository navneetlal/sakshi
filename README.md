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
- **Native inference.** Speech recognition and speaker diarization run in
  NVIDIA's NeMo-Speech.cpp runtime as a sidecar process. No Python.

## Status

Early development. See [DECISIONS.md](DECISIONS.md) for design decisions.

| Milestone | Scope | State |
|---|---|---|
| M0 | Workspace, CI, docs | in progress |
| M1 | `sakshi-cli process <folder>` on existing recordings | planned |
| M2 | SQLite store and MCP server | planned |
| M3 | macOS capture | planned |
| M4 | Desktop app (Tauri 2) | planned |
| M5 / M6 | Windows / Linux capture | planned |

## Building

Requires the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs
it automatically).

```bash
cargo build
cargo test
```

## License

Sakshi's code is licensed under the [MIT License](LICENSE). Models are
downloaded separately and keep their own licenses, which Sakshi shows before
each download.
