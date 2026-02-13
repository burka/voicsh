---
title: Installation
weight: 2
---

## Build dependencies

Rust (via [rustup](https://rustup.rs/)) plus a C toolchain, cmake, pkg-config, libclang, and ALSA headers:

```bash
# Debian/Ubuntu:
sudo apt install build-essential cmake pkg-config libclang-dev libasound2-dev

# Fedora:
sudo dnf install gcc gcc-c++ cmake pkg-config clang-devel alsa-lib-devel

# Arch:
sudo pacman -S base-devel cmake pkgconf clang alsa-lib
```

For the authoritative list, see [`test-containers/Dockerfile.vulkan`](test-containers/Dockerfile.vulkan).

## Install

```bash
cargo install voicsh
```

### Pipe-only build (no microphone)

Skip the ALSA dependency if you only need WAV-to-text:

```bash
cargo install voicsh \
    --no-default-features --features cli,portal,model-download
```

## GPU acceleration

By default voicsh runs on CPU. Enable GPU for ~5-10x faster transcription:

| Backend | Flag | Prerequisites |
|---------|------|---------------|
| NVIDIA  | `--features cuda` | [CUDA Toolkit](https://developer.nvidia.com/cuda-toolkit) ≥ 11.0 |
| Cross-platform | `--features vulkan` | [Vulkan SDK](https://vulkan.lunarg.com/) — on Ubuntu: `libvulkan-dev mesa-vulkan-drivers vulkan-tools glslc` |
| AMD (discrete) | `--features hipblas` | [ROCm](https://rocm.docs.amd.com/) |
| CPU optimized | `--features openblas` | `libopenblas-dev` / `openblas` |

Verify with `voicsh check` (shows detected GPU hardware and compiled backend).

## Runtime dependencies (mic mode only)

Text injection needs one of:
- **Nothing extra** on GNOME 45+ / KDE 6.1+ (uses xdg-desktop-portal)
- `wtype` for wlroots compositors (Sway, Hyprland)
- `ydotool` + `ydotoold` as fallback

`wl-clipboard` (`wl-copy`) is required for clipboard access.

```bash
voicsh check    # verify what's available
voicsh init     # auto-detect and configure the best backend
```

Pipe mode (`cat file.wav | voicsh`) has no runtime dependencies.
