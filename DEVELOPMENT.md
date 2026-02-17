---
title: Development
weight: 4
---

## Build Prerequisites

Requires Rust (via rustup), a C toolchain, cmake, pkg-config, clang, libclang, and ALSA development headers:

```bash
# Debian/Ubuntu:
sudo apt install build-essential cmake pkg-config clang libclang-dev libasound2-dev

# Fedora:
sudo dnf install gcc gcc-c++ cmake pkg-config clang clang-devel alsa-lib-devel

# Arch:
sudo pacman -S base-devel cmake pkgconf clang alsa-lib
```

**Why `clang` (the binary)?** GPU builds use `bindgen` to generate FFI bindings. `bindgen` loads `libclang` as a library, but needs the `clang` binary in PATH to locate its resource directory (containing `stdbool.h`, `stddef.h`, etc.). Without it, `bindgen` silently falls back to pre-built bindings that lack GPU symbols. Some distros install only a versioned binary (e.g. `clang-20`) via `libclang-dev` — `bindgen` only looks for the unversioned `clang`.

For the authoritative list of system dependencies, see [`test-containers/Dockerfile.vulkan`](test-containers/Dockerfile.vulkan).

### Runtime Dependencies

Text injection requires `wl-clipboard` plus either `wtype` (wlroots) or `ydotool` (fallback):

```bash
# Debian/Ubuntu:
sudo apt install wl-clipboard wtype

# Fedora:
sudo dnf install wl-clipboard wtype

# Arch:
sudo pacman -S wl-clipboard wtype
```

### GPU Backend Dependencies

GPU backends are optional. Add their system packages before building with `--features <backend>`.

| Backend | Feature flag | System packages (Debian/Ubuntu) |
|---------|-------------|--------------------------------|
| Vulkan | `vulkan` | `libvulkan-dev mesa-vulkan-drivers vulkan-tools glslc` |
| CUDA | `cuda` | NVIDIA driver + [CUDA Toolkit](https://developer.nvidia.com/cuda-downloads) (`nvcc`) |
| HipBLAS | `hipblas` | [ROCm](https://rocm.docs.amd.com/) (`rocminfo`) |
| OpenBLAS | `openblas` | `libopenblas-dev` |

```bash
# Vulkan (Intel/AMD/NVIDIA — Debian/Ubuntu):
sudo apt install libvulkan-dev mesa-vulkan-drivers vulkan-tools glslc

# CUDA (NVIDIA only — see link above for toolkit install):
# Requires: nvidia-smi, nvcc

# OpenBLAS (CPU-only BLAS optimization):
sudo apt install libopenblas-dev
```

**GPU build troubleshooting:** If you see `unresolved import ggml_backend_vk_*` errors, bindgen failed to generate GPU bindings and fell back to incomplete pre-built ones. Common causes and fixes:

| Symptom | Cause | Fix |
|---------|-------|-----|
| `stdbool.h` not found | `clang` binary not in PATH | `sudo apt install clang` |
| Versioned clang exists (e.g. `clang-20`) but `clang` doesn't | Distro only installed versioned binary | `sudo apt install clang` or `export CLANG_PATH=$(which clang-20)` |
| No clang at all | libclang-dev not installed | `sudo apt install clang libclang-dev` |

The build script runs preflight checks and will panic with specific guidance before compilation starts. If you need to bypass the check: `export CLANG_PATH=/usr/bin/clang-20`.

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
