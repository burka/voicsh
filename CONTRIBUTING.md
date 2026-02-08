# Contributing to voicsh

voicsh is a free-time side project. Contributions of all kinds are welcome — from bug reports to code changes to just telling me it didn't build on your machine.

Maintenance time is limited, so the most helpful contributions are ones that are **easy to review**: small, focused, and well-described. A PR that does one thing with a clear explanation gets merged fast. A vague issue titled "it doesn't work" might sit for a while.

### Before opening an issue

- **Build problems or usage questions**: try `voicsh check`, check the README, and consider asking Claude Code with the source — it knows this codebase well
- **Bug reports**: great, but include enough context to reproduce (see template below)
- **Feature requests**: check [ROADMAP.md](ROADMAP.md) first — it might already be planned

### What makes a great contribution

- **Concise PRs** — one logical change per PR, with a short description of what and why
- **Actionable issues** — describe what you did, what happened, and what you expected
- **Test reports** — "I ran it on [hardware/distro/compositor] and [result]" is genuinely useful

## Ways to contribute

### Report bugs or build failures

Open an issue. Include:
- Distro and version (e.g., Fedora 41, Arch rolling)
- Wayland compositor (GNOME, KDE, Sway, Hyprland, etc.)
- Rust version (`rustc --version`)
- What happened vs. what you expected
- Output of `voicsh check` if applicable

"It didn't compile" or "it crashed immediately" are useful reports at this stage.

### Test on your hardware

The project has only been tested on a single developer machine. Running voicsh on different hardware helps enormously:
- Different audio devices / sample rates
- GPU backends (CUDA, Vulkan, ROCm) — these are largely untested
- Wayland compositors beyond GNOME and Sway
- Non-English languages

### Improve accuracy

The transcription pipeline (VAD, chunking, Whisper parameters) has plenty of room for improvement. If you have ideas or experience with speech recognition, I'd appreciate the help.

### Code contributions

PRs are welcome. Before submitting:

```bash
cargo fmt
cargo clippy --lib --no-default-features --features cli,portal,model-download -- -D warnings
cargo build  --lib --no-default-features --features cli,portal,model-download
cargo test   --lib --no-default-features --features cli,portal,model-download
cargo test   --lib --no-default-features --features portal
cargo test   --lib --no-default-features
```

If you have ALSA headers and cmake installed, also run the full build:

```bash
cargo fmt
cargo clippy -- -D warnings
cargo build
cargo test
```

Quality bar: **0 errors, 0 warnings, 0 test failures.** See [CLAUDE.md](CLAUDE.md) for the full set of rules.

### Suggest features

Open an issue describing the use case. Check [ROADMAP.md](ROADMAP.md) first — it might already be planned.

## Development setup

See [DEVELOPMENT.md](DEVELOPMENT.md) for build prerequisites, testing commands, and mocking patterns.

## Code style

- `cargo fmt` — no exceptions
- Tests must assert concrete values (not just `is_ok()`)
- Errors must tell the user what to do next
- Keep it simple — no over-engineering

## License

By contributing you agree that your contributions are licensed under the MIT License.
