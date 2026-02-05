# voicsh Development Guidelines

## Quality Gates (Non-Negotiable)
- **0 errors** · **0 warnings** · **0 test failures**

## Commands Before Every Commit
```bash
cargo fmt
cargo clippy --lib --no-default-features --features portal -- -D warnings
cargo test --lib --no-default-features --features portal
cargo test --lib --no-default-features
```

## Documentation Rules
- Each .md file has **one purpose** — don't duplicate content, reference other files
- Document **current state** or **desired state** — no history, changelogs, or progress reports
- Keep docs concise; prefer code references over prose

## References
- [ARCHITECTURE.md](ARCHITECTURE.md) — System design, pipeline, components
- [DEVELOPMENT.md](DEVELOPMENT.md) — Testing, mocking patterns, quality checks
- [ROADMAP.md](ROADMAP.md) — Phases and planned features
