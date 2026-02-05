# voicsh — Voice typing for Wayland Linux

Offline, privacy-first voice typing. Speak into your mic, text appears in your focused app. Or pipe a WAV file and get text on stdout.

## Usage

```bash
voicsh                          # continuous mic → inject text into focused app
voicsh --once                   # single utterance → inject → exit
voicsh --model small.en -v      # override model, show volume meter
voicsh --fan-out                # run English + multilingual models, pick best
cat file.wav | voicsh           # transcribe WAV from stdin → stdout

voicsh devices                  # list audio input devices
voicsh models list              # available models
voicsh models install base.en   # download a model
voicsh check                    # verify system dependencies
```

### Verbosity

- No flag: text only
- `-v`: volume meter + `[ok 43ch] "text"` result lines
- `-vv`: full diagnostics (chunk timing, paste detection)

## How it works

```
Mic/WAV → VAD → Chunker → Whisper → Text injection
                                      ↓
                              portal / wtype / ydotool
```

1. Audio captured via cpal (mic) or hound (WAV file)
2. Voice activity detection splits speech into chunks
3. whisper-rs transcribes each chunk locally
4. Text injected via xdg-desktop-portal (GNOME/KDE), wtype, or ydotool

Pipe mode (`cat file.wav | voicsh`) skips injection and writes to stdout.

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

Plus `wl-clipboard` (`wl-copy`) for clipboard access.

```bash
voicsh check    # verify what's available
```

Pipe mode (`cat file.wav | voicsh`) has no runtime dependencies beyond the binary.

## Models

| Model | Size | Speed | Use case |
|-------|------|-------|----------|
| `tiny.en` | 75 MB | Fastest | Quick notes, low-end hardware |
| `base.en` | 142 MB | Fast | **Default — good balance** |
| `small.en` | 466 MB | Medium | When accuracy matters |
| `medium.en` | 1.5 GB | Slow | Professional transcription |
| `base` | 142 MB | Fast | Multilingual (auto-detect) |

English-only models (`.en`) are faster and more accurate for English. Use `--language auto` with multilingual models for other languages.

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
method = "clipboard"
paste_key = "auto"          # auto-detects terminal vs GUI
```

Environment overrides: `VOICSH_MODEL=small.en voicsh`

## Wayland compatibility

Tested on GNOME, KDE Plasma, Sway, Hyprland.

## License

MIT

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Whisper inference engine
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- Inspired by [nerd-dictation](https://github.com/ideasman42/nerd-dictation), [voxd](https://github.com/voxd/voxd), and [BlahST](https://github.com/QuantiusBenignus/BlahST)
