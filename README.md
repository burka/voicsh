# voicsh — Voice typing for Wayland Linux

Offline, privacy-first voice typing. Speak into your mic, text appears in your focused app. Or pipe a WAV file and get text on stdout.

**Build:** Rust, C compiler, cmake, pkg-config, libclang, ALSA headers — `sudo apt install build-essential cmake pkg-config libclang-dev libasound2-dev`
**Run (mic mode):** `sudo apt install wl-clipboard wtype ydotool` — GNOME 45+ / KDE 6.1+ work without wtype/ydotool; pipe mode has no runtime deps

> **Status: Early MVP (v0.0.1)**
>
> Free-time side project. Primary target: **Ubuntu + GNOME + Wayland** — that's what I develop and test on. Other distros, desktops, and compositors are welcome, but I can't reproduce issues outside this setup. Maintenance time is limited.
>
> If something doesn't work, [open an issue](https://github.com/burka/voicsh/issues) so we can improve it together. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to make the most of limited maintenance bandwidth.

## Quick start

```bash
cargo install --git https://github.com/burka/voicsh.git

# Test with a WAV file first (no mic or runtime deps needed):
cat file.wav | voicsh

# Mic mode (requires runtime deps below):
voicsh                          # continuous mic → text into focused app
voicsh --once                   # single utterance → exit

# Auto-tune: benchmark hardware, pick best model:
voicsh init

# For all commands and options:
voicsh help
```

## How it works

```
Mic/WAV → VAD → Chunker → Whisper → Post-processor → Text injection
                                                        ↓
                                                portal / wtype / ydotool
```

1. Audio captured via cpal (mic) or hound (WAV file)
2. Voice activity detection splits speech into chunks
3. whisper-rs transcribes each chunk locally
4. Text injected via xdg-desktop-portal (GNOME/KDE), wtype, or ydotool

Pipe mode (`cat file.wav | voicsh`) skips injection and writes to stdout.

## Install

### Build dependencies

Rust (via [rustup](https://rustup.rs/)) plus a C toolchain, cmake, pkg-config, libclang, and ALSA headers:

```bash
# Debian/Ubuntu:
sudo apt install build-essential cmake pkg-config libclang-dev libasound2-dev

# Fedora:
sudo dnf install gcc gcc-c++ cmake pkg-config clang-devel alsa-lib-devel

# Arch:
sudo pacman -S base-devel cmake pkgconf clang alsa-lib
```

For the authoritative list of system dependencies, see [`test-containers/Dockerfile.vulkan`](test-containers/Dockerfile.vulkan).

```bash
cargo install --git https://github.com/burka/voicsh.git
```

If you only need pipe mode (WAV → text, no microphone) and want to skip the ALSA dependency:

```bash
cargo install --git https://github.com/burka/voicsh.git \
    --no-default-features --features cli,portal,model-download
```

### GPU acceleration

By default voicsh runs on CPU. Enable GPU for ~5-10x faster transcription:

| Backend | Flag | Prerequisites |
|---------|------|---------------|
| NVIDIA  | `--features cuda` | [CUDA Toolkit](https://developer.nvidia.com/cuda-toolkit) ≥ 11.0 |
| Cross-platform | `--features vulkan` | [Vulkan SDK](https://vulkan.lunarg.com/) — on Ubuntu: `libvulkan-dev mesa-vulkan-drivers vulkan-tools glslc` |
| AMD (discrete) | `--features hipblas` | [ROCm](https://rocm.docs.amd.com/) |
| CPU optimized | `--features openblas` | `libopenblas-dev` / `openblas` |

Verify with `voicsh check` (shows detected GPU hardware and compiled backend).

### Runtime dependencies (mic mode only)

Text injection needs **one of**:
- **Nothing extra** on GNOME 45+ / KDE 6.1+ (uses xdg-desktop-portal)
- `wtype` for wlroots compositors (Sway, Hyprland)
- `ydotool` + `ydotoold` as fallback

`wl-clipboard` (`wl-copy`) is required for clipboard access.

```bash
voicsh check    # verify what's available
```

Pipe mode (`cat file.wav | voicsh`) has no runtime dependencies beyond the binary.

## Voice commands

Voice commands trigger only when spoken as **standalone utterances** — pause, say the command, pause. Text that merely contains a command word passes through unchanged:

```
[pause] "period" [pause]          → .
[pause] "new line" [pause]        → (line break)
"the period of history"           → "the period of history"
"press enter to continue"        → "press enter to continue"
[pause] "all caps" [pause] "wow" → "WOW"
```

Built-in commands are available for English, German, Spanish, French, Portuguese, Italian, Dutch, Polish, Russian, Japanese, Chinese, and Korean. Discover all commands for a language:

```bash
voicsh config list --language=en     # English voice commands
voicsh config list --language=ko     # Korean voice commands
voicsh config list --language=en,de  # multiple languages
```

Add custom commands in `[voice_commands.commands]` in config — they take precedence over built-ins. To disable voice commands entirely: `voice_commands.enabled = false`.

## Configuration

```bash
voicsh config dump              # commented template with all options and defaults
voicsh config list              # current active configuration
voicsh config list stt          # just the [stt] section
voicsh config get stt.model     # single value
voicsh config set stt.model small.en
```

Config file: `~/.config/voicsh/config.toml`. Environment overrides: `VOICSH_MODEL=small.en voicsh`.

## Shell integration

**GNOME extension** — panel indicator with recording state, model info, and Super+Alt+V toggle:

```bash
voicsh install-gnome-extension
```

**Shell completions:** `voicsh completions bash|zsh|fish` — run `voicsh completions --help` for install paths.

## License

MIT

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Whisper inference engine
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- Inspired by [nerd-dictation](https://github.com/ideasman42/nerd-dictation), [voxd](https://github.com/voxd/voxd), and [BlahST](https://github.com/QuantiusBenignus/BlahST)
