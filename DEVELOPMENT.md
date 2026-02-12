# voicsh Development Guide

## Build Prerequisites

Requires Rust (via rustup), cmake, pkg-config, and ALSA development headers. On Debian/Ubuntu: `sudo apt install cmake pkg-config libasound2-dev`. On Fedora: `sudo dnf install cmake pkg-config alsa-lib-devel`. For runtime text injection, install `wl-clipboard` plus either `wtype` (wlroots) or `ydotool` (fallback).

## Local Install

```bash
cargo install --path=.                   # CPU only
cargo install --features=cuda --path=.   # NVIDIA GPU
cargo install --features=vulkan --path=. # Vulkan GPU
```

Installs to `~/.cargo/bin/voicsh`.

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

## GNOME Extension Development

The extension lives in `gnome/voicsh@voicsh.dev/`.

### Nested Shell (UI and IPC iteration)

A dev script launches a **nested GNOME Shell** inside a window — fast iteration without restarting your session.

```bash
./gnome/dev.sh              # launch nested shell with extension enabled
./gnome/dev.sh --verbose    # same, with full GLib/Shell debug output
```

The script handles symlinks, schema compilation, and auto-enabling the extension. Edit code, close the nested window, rerun — ~2-3 second cycle.

Good for: icon states, menu rendering, styles, IPC (socket connect, toggle, status polling), keybindings.

**Not for end-to-end text injection.** The voicsh daemon runs on the host compositor. The nested shell is a separate Wayland compositor — injected text goes to the host, not into the nested window. Test text injection in your real session.

**Why not disable/re-enable?** GJS caches imported JS modules in memory. `gnome-extensions disable && enable` re-runs `enable()` on the cached code — it does not re-read files from disk. A fresh nested shell process has no cache.

**Requirements**: `gnome-shell`, `glib-compile-schemas`, and on GNOME 49+ also `mutter-dev-bin` (`sudo apt install mutter-dev-bin`). Uses `--devkit` on GNOME 49+, `--nested` on 45-48.

### Real Session (end-to-end testing)

For testing text injection, install the extension in your real session and log out/in:

```bash
# Symlink is already set up by dev.sh, just enable in real session:
gnome-extensions enable voicsh@voicsh.dev
# Log out and back in to load the extension
```

Code changes require a session restart (log out/in) due to GJS module caching.

## Quality Checks

See [CLAUDE.md](CLAUDE.md) for the canonical quality gate commands to run before every commit.
