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

## Test Rules — Every Test Must Validate Its Outcome
- A test only counts if it **validates the result against an expected value**
- `assert!(x.is_ok())` alone is **never sufficient** — unwrap and `assert_eq!` on the value
- `assert!(x.is_err())` alone is **never sufficient** — verify the error variant or message
- `assert!(x.is_some())` alone is **never sufficient** — unwrap and check the inner value
- After unwrapping, assert the concrete value (`assert_eq!`, not just `assert!`)
- "Doesn't panic" tests must document why in a comment
- A test without outcome validation does not count toward coverage goals

## Linting Rules — Zero Tolerance
- `cargo clippy -- -D warnings` must pass with **0 warnings** before every commit
- `cargo fmt` must produce no changes before every commit
- Treat clippy suggestions as requirements, not suggestions
- Never suppress clippy lints without a justifying comment

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

## GNOME Extension Development
```bash
./gnome/dev.sh              # nested GNOME Shell with extension auto-enabled
./gnome/dev.sh --verbose    # with debug output
```
Handles symlink, schema compilation, GNOME 45-49+ flag detection. See [DEVELOPMENT.md](DEVELOPMENT.md) for details.

## References
- [ARCHITECTURE.md](ARCHITECTURE.md) — System design, pipeline, components
- [DEVELOPMENT.md](DEVELOPMENT.md) — Testing, mocking patterns, quality checks
- [ROADMAP.md](ROADMAP.md) — Phases and planned features
