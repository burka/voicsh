# voicsh Development Guidelines

## Quality Gates (Non-Negotiable)
- **0 errors** · **0 warnings** · **0 test failures**

## Commands Before Every Commit
```bash
cargo fmt
cargo clippy -- -D warnings
cargo build
cargo test
cargo test --lib --no-default-features --features portal
cargo test --lib --no-default-features
```

> `default = ["full"]` pulls in `cpal-audio` (ALSA) and `whisper` (cmake).
> If ALSA/cmake are unavailable (CI, containers, remote agents), use this reduced set:
> ```bash
> cargo fmt
> cargo clippy --lib --no-default-features --features cli,portal,model-download -- -D warnings
> cargo build  --lib --no-default-features --features cli,portal,model-download
> cargo test --lib   --no-default-features --features cli,portal,model-download
> cargo test --lib  --no-default-features --features portal
> cargo test --lib  --no-default-features
> ```

## Test Rules
- Every test MUST assert expected values, not just `is_ok()` / `is_some()`
- After unwrapping, assert the concrete value (`assert_eq!`, not just `assert!`)
- `is_err()` checks must also verify the error variant or message
- "Doesn't panic" tests must document why in a comment
- A test without outcome validation does not count toward coverage goals

## Error Handling Rules — Fail Fast, Fail Loud, Fail Helpful
- Never silently discard errors (`let _ =` on Result is forbidden outside tests)
- If an error means we can't do our job → exit with a helpful message
- Cleanup/shutdown errors → `eprintln!` with context (not silent)
- Every error message must tell the user what to do next

## Documentation Rules
- Each .md file has **one purpose** — don't duplicate content, reference other files
- Document **current state** or **desired state** — no history, changelogs, or progress reports
- No status data in docs: no test counts, no build stats, no "X passed" — these go stale instantly
- Keep docs concise; prefer code references over prose

## References
- [ARCHITECTURE.md](ARCHITECTURE.md) — System design, pipeline, components
- [DEVELOPMENT.md](DEVELOPMENT.md) — Testing, mocking patterns, quality checks
- [ROADMAP.md](ROADMAP.md) — Phases and planned features
