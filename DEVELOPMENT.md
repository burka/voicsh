# voicsh Development Guide

## Build Prerequisites

Requires Rust (via rustup), cmake, pkg-config, and ALSA development headers. On Debian/Ubuntu: `sudo apt install cmake pkg-config libasound2-dev`. On Fedora: `sudo dnf install cmake pkg-config alsa-lib-devel`. For runtime text injection, install `wl-clipboard` plus either `wtype` (wlroots) or `ydotool` (fallback).

## Testing

### Commands

```bash
# Fast (no whisper-rs compilation):
cargo test --lib --no-default-features --features portal
cargo test --lib --no-default-features

# Full (slow, compiles whisper-rs):
cargo test --lib
```

The `--lib` flag is required — integration tests need a real model file.

### Mocking Pattern

All external dependencies are behind traits with test doubles:

| Trait | Production | Test |
|-------|-----------|------|
| `AudioSource` | `CpalAudioSource`, `WavAudioSource` | `MockAudioSource` |
| `Transcriber` | `WhisperTranscriber` | `MockTranscriber` |
| `CommandExecutor` | `SystemCommandExecutor` | `MockCommandExecutor` / `RecordingExecutor` |
| `TextSink` | `InjectorSink`, `StdoutSink` | `CollectorSink` |
| `PortalConnector` | `AshpdConnector` | inline test connector |

### Conventions

- Unit tests live in `#[cfg(test)] mod tests` inside each file
- `serde_json` is available as a dependency for JSON test assertions
- `tempfile` is available for filesystem tests

## Quality Checks

```bash
cargo fmt
cargo build --release                                        # catches daemon + feature-gated code
cargo clippy --lib --no-default-features --features portal -- -D warnings
cargo test --lib --no-default-features --features portal
cargo test --lib --no-default-features
```

Run all five before every commit. The `--release` build is essential — fast tests use `--no-default-features` which skips the daemon module and whisper integration. See [CLAUDE.md](CLAUDE.md) for quality gates.
