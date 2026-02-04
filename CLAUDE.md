# voicsh Development Guidelines

## Quality Gates (Non-Negotiable)
- **0 errors** · **0 warnings** · **0 test failures** · **>80% coverage**

## Workflow
`plan → minimal solution → code → test → verify (SOLID/DRY/SRP) → lint+format → commit`

## Commands Before Every Commit
```bash
cargo fmt && cargo clippy -- -D warnings && cargo test
```

## References
- [ARCHITECTURE.md](ARCHITECTURE.md) - Components, data flow, technology choices
- [ROADMAP.md](ROADMAP.md) - MVP scope, phases, success criteria
- [DEVELOPMENT.md](DEVELOPMENT.md) - TDD approach, test strategy, agent assignments
