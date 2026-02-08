# voicsh — Voice typing for Wayland Linux

Offline, privacy-first voice typing. Speak into your mic, text appears in your focused app. Or pipe a WAV file and get text on stdout.

> **Status: Early MVP (v0.0.1-dev)**
>
> This is a free-time side project developed on **Ubuntu + GNOME + Wayland**. The current goal is to make it work out of the box on that setup, including GPU acceleration. Other distros and desktops are welcome — I just can't test them or reproduce issues myself. Maintenance time is limited — see [CONTRIBUTING.md](CONTRIBUTING.md) for how to make the most of it.
>
> **What works:**
> - CPU transcription via whisper.cpp — functional, accuracy varies by model and environment
> - Pipe mode (`cat file.wav | voicsh`) — most reliable path
> - Daemon mode with IPC control
> - Text injection on GNOME/KDE via xdg-desktop-portal
> - Voice commands (punctuation, formatting)
>
> **What doesn't (yet):**
> - GPU acceleration — feature-gated but largely untested; expect build or runtime issues
> - Developed on Ubuntu + GNOME — other distros/compositors may need tweaks
> - VAD tuning is basic; background noise or quiet speech may cause missed/false chunks
>
> **What I'd love help with:**
> - Testing on different hardware, distros, and compositors
> - GPU backend testing (CUDA, Vulkan, ROCm)
> - Improving transcription accuracy (VAD tuning, chunking strategy)
> - Bug reports — even "it didn't build" is valuable at this stage
>
> If any of this sounds interesting, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Usage

```bash
voicsh                          # continuous mic → inject text into focused app
voicsh --once                   # single utterance → inject → exit
voicsh --model small.en -v      # override model, show volume meter
voicsh --fan-out                # run English + multilingual models, pick best
cat file.wav | voicsh           # transcribe WAV from stdin → stdout

voicsh init                     # auto-tune: benchmark, pick best model, configure
voicsh devices                  # list audio input devices
voicsh models list              # available models
voicsh models install base.en   # download a model
voicsh check                    # verify system dependencies


# Daemon mode
voicsh daemon                   # start long-running daemon (model stays in memory)
voicsh start                    # tell daemon to start recording
voicsh stop                     # tell daemon to stop recording
voicsh toggle                   # toggle recording on/off
voicsh status                   # show daemon health (recording state, model info)
voicsh install-service          # install systemd user service
```

### Verbosity

- No flag: text only
- `-v`: volume meter + `[ok 43ch] "text"` result lines
- `-vv`: full diagnostics (chunk timing, paste detection)

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

### Daemon mode

For lower latency, run voicsh as a daemon. The Whisper model stays loaded in memory (~300 MB for base.en), so subsequent recordings start instantly.

```bash
voicsh daemon                   # start daemon (foreground)
voicsh install-service          # install as systemd user service
systemctl --user enable --now voicsh
```

Control via IPC (Unix socket):

```bash
voicsh start                    # start recording
voicsh stop                     # stop and inject transcription
voicsh toggle                   # toggle recording on/off
voicsh status                   # check daemon health
```

## Install

### Build from source

```bash
git clone https://github.com/burka/voicsh.git
cd voicsh
cargo build --release
cp target/release/voicsh ~/.local/bin/
```

### GPU acceleration

By default voicsh runs on CPU. Enable GPU for ~5-10x faster transcription:

| Backend | Flag | Prerequisites |
|---------|------|---------------|
| NVIDIA  | `--features cuda` | [CUDA Toolkit](https://developer.nvidia.com/cuda-toolkit) ≥ 11.0 |
| Cross-platform | `--features vulkan` | [Vulkan SDK](https://vulkan.lunarg.com/) |
| AMD | `--features hipblas` | [ROCm](https://rocm.docs.amd.com/) |
| CPU optimized | `--features openblas` | `libopenblas-dev` / `openblas` |

```bash
cargo build --release --features cuda     # NVIDIA GPU
cargo build --release --features vulkan   # Any GPU with Vulkan
cargo build --release --features hipblas  # AMD GPU
cargo build --release --features openblas # Faster CPU (BLAS)
```

Verify with `voicsh check` (shows detected GPU hardware and compiled backend)
or `voicsh -v` (shows backend on startup).

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

## Quick start: `voicsh init`

The fastest way to get started. `voicsh init` benchmarks your hardware and picks the best model automatically:

```bash
voicsh init                     # auto-detect language, pick best multilingual model
voicsh init --language en       # optimize for English (picks .en models)
voicsh init --language de       # German — picks multilingual model
```

What it does:

1. Detects your CPU, RAM, disk space, and GPU
2. Downloads a tiny probe model (75 MB) and benchmarks it
3. Fetches the model list from HuggingFace (metadata only — names and sizes, not the model files)
4. Estimates performance for every model using a size-ratio formula
5. Picks the highest-quality model that runs faster than real-time on your hardware
6. Downloads and verifies the recommended model
7. Saves the choice to `~/.config/voicsh/config.toml`

Run `voicsh init` again anytime to re-tune (e.g., after a hardware upgrade).

### How the estimation works

Speed scales linearly with model file size: `estimated_RTF = probe_RTF × (model_size / probe_size)`. This works because file size is proportional to parameter count, and compute is proportional to parameters. Quantized models (q4_0, q5_1, etc.) have smaller files and proportionally faster inference, so the formula handles them automatically.

After downloading the recommended model, `init` runs a verification benchmark and falls back to a smaller model if the estimate was too optimistic.

## Models

| Model | Size | Speed | Use case |
|-------|------|-------|----------|
| `tiny` | 75 MB | Fastest | Quick notes, low-end hardware |
| `base` | 142 MB | Fast | **Default — good balance** |
| `small` | 466 MB | Medium | When accuracy matters |
| `medium` | 1.5 GB | Slow | Professional transcription |
| `large` | 1.6 GB | Slowest | Highest accuracy (large-v3-turbo) |

All models support auto-language detection. English-only variants (`.en` suffix, e.g., `base.en`) are faster and more accurate for English.

Quantized variants (`-q4_0`, `-q5_1`, etc.) are available via `voicsh models list` — smaller files, faster inference, slightly reduced accuracy. `voicsh init` considers all variants when picking the best model for your hardware.

## Configuration

`~/.config/voicsh/config.toml`:

```toml
[audio]
vad_threshold = 0.02
silence_duration_ms = 1500

[stt]
model = "base"
language = "auto"

[input]
method = "Clipboard"
paste_key = "auto"          # auto-detects terminal vs GUI

[voice_commands]
enabled = true              # enable spoken punctuation/formatting (default: true)

[voice_commands.commands]   # add or override voice commands
"smiley" = ":)"
"at sign" = "@"
```

Environment overrides: `VOICSH_MODEL=small.en voicsh`

## Voice commands

Spoken punctuation and formatting are processed automatically. Say "period" and get `.`, say "new line" and get a line break.

Built-in commands (English):

| Spoken phrase | Output |
|---------------|--------|
| period / full stop | `.` |
| comma | `,` |
| question mark | `?` |
| exclamation mark | `!` |
| colon | `:` |
| semicolon | `;` |
| new line | `\n` |
| new paragraph | `\n\n` |
| all caps | toggle uppercase on |
| end caps | toggle uppercase off |
| open quote / close quote | `"` |
| open parenthesis / close parenthesis | `(` `)` |
| dash | ` — ` |
| hyphen | `-` |
| ellipsis | `...` |
| tab | `\t` |

Built-in commands are also available for German, Spanish, French, Portuguese, Italian, Dutch, Polish, Russian, Japanese, Chinese, and Korean. The language is selected from `stt.language` in config.

Add custom commands in `[voice_commands.commands]` — they take precedence over built-ins. Set `voice_commands.enabled = false` to disable.

## Wayland compatibility

**Primary target:** Ubuntu + GNOME + Wayland — this is what I develop and test on.

Community-reported as working on KDE Plasma, Sway, and Hyprland. Other distros and compositors should work but are not tested by the author — reports and fixes are welcome.

## License

MIT

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Whisper inference engine
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- Inspired by [nerd-dictation](https://github.com/ideasman42/nerd-dictation), [voxd](https://github.com/voxd/voxd), and [BlahST](https://github.com/QuantiusBenignus/BlahST)
