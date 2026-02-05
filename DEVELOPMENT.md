# voicsh Development Guide

Test-driven development workflow for building voicsh.

## Core Principles

1. **Tests First**: Write failing test → implement → pass → refactor
2. **Minimal Code**: Simplest solution that passes tests
3. **Trait Abstraction**: Mock external dependencies (audio, subprocess, whisper)
4. **Small Commits**: One logical change per commit

## Component Build Order

Build in dependency order (most testable first):

| Order | Component | Testability | Agent |
|-------|-----------|-------------|-------|
| 1 | `config.rs` | HIGH - pure parsing | developer |
| 2 | `audio/vad.rs` | HIGH - pure math | developer |
| 3 | `models/catalog.rs` | HIGH - pure data | junior |
| 4 | `ipc/protocol.rs` | HIGH - serialization | junior |
| 5 | `error.rs` | HIGH - types only | junior |
| 6 | `audio/recorder.rs` | MEDIUM - trait mock | developer |
| 7 | `stt/whisper.rs` | MEDIUM - trait mock | developer |
| 8 | `input/injector.rs` | MEDIUM - mock subprocess | developer |
| 9 | `pipeline/sink.rs` | MEDIUM - trait mock | developer |
| 10 | `ipc/server.rs` | MEDIUM - async socket | developer |
| 11 | `cli.rs` | MEDIUM - arg parsing | junior |
| 12 | `main.rs` | LOW - integration | developer |

## Test Strategy

### Unit Tests (in-file `#[cfg(test)]`)
- Pure functions, parsing, validation
- Use mocks for external dependencies
- Target: 70-80% of all tests

### Integration Tests (`tests/` directory)
- Full pipeline, IPC, file I/O
- Use `tempfile` for isolation
- Mark slow tests with `#[ignore]`

### Mocking Pattern

All external dependencies behind traits:

```
AudioSource      → MockAudioSource (returns predefined samples)
Transcriber      → MockTranscriber (returns predefined text)
CommandExecutor  → RecordingExecutor (captures calls, verifies args)
TextSink         → CollectorSink (accumulates text, returns on finish())
```

## Quality Checks

### Before Every Commit
```bash
cargo build --no-default-features --lib  # Verify lib compiles without heavy deps
cargo fmt && cargo clippy -- -D warnings && cargo test
```

### CI Pipeline
1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo test` (unit)
4. `cargo test -- --ignored` (integration, needs model)
5. `cargo audit`
6. Coverage report (>80%)

## Test Fixtures

```
tests/fixtures/
├── audio/
│   ├── silence.wav      # VAD testing
│   └── hello.wav        # Transcription testing
└── config/
    ├── valid.toml       # Config parsing
    └── invalid.toml     # Error handling
```

## Dev Dependencies

```toml
[dev-dependencies]
tempfile = "3"           # Temp directories
rstest = "0.18"          # Parameterized tests
proptest = "1.4"         # Property-based tests
assert_cmd = "2.0"       # CLI testing
mockall = "0.12"         # Complex mocks (if needed)
```

## Agent Responsibilities

| Agent | Use For |
|-------|---------|
| **junior** | Boilerplate, module structure, simple data types, CLI args |
| **developer** | Complex logic, trait design, error handling, integrations |
| **tester** | Additional edge cases, fuzz tests, coverage gaps |
| **architect** | Before new phases, review trait design, evaluate patterns |

## Commit Message Format

```
<type>: <description>

Tests: X passed, Y% coverage
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
