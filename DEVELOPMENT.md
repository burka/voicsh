# voicsh Development Guide

## Testing

### Commands

```bash
# Fast (no whisper-rs compilation):
cargo test --lib --no-default-features --features portal   # 302 tests
cargo test --lib --no-default-features                      # 294 tests (no portal)

# Full (slow, compiles whisper-rs):
cargo test --lib
```

The `--lib` flag is required — 2 integration tests need a real model file.

### Mocking Pattern

All external dependencies are behind traits with test doubles:

| Trait | Production | Test |
|-------|-----------|------|
| `AudioSource` | `CpalAudioSource` | `MockAudioSource` |
| `Transcriber` | `WhisperTranscriber` | `MockTranscriber` |
| `CommandExecutor` | `SystemCommandExecutor` | `MockCommandExecutor` / `RecordingExecutor` |
| `TextSink` | `InjectorSink` | `CollectorSink` |

### Conventions

- Unit tests live in `#[cfg(test)] mod tests` inside each file
- `serde_json` is available as a dependency for JSON test assertions
- `tempfile` is available for filesystem tests

## Quality Checks

```bash
cargo fmt
cargo clippy --lib --no-default-features --features portal -- -D warnings
cargo test --lib --no-default-features --features portal
cargo test --lib --no-default-features
```

Run all four before every commit. See [CLAUDE.md](CLAUDE.md) for quality gates.
