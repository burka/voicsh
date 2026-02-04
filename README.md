# voic.sh - Voice Input for Wayland Linux

System-wide, privacy-focused voice typing for Wayland Linux with offline-first AI processing, real-time feedback, and optional text refinement.

## Features

- **Offline-First**: All core functionality works without internet
- **Privacy-Focused**: Audio never leaves your machine (unless you opt into cloud LLM refinement)
- **Wayland-Native**: Built for modern Linux desktops with layer-shell overlay
- **Low Latency**: <500ms from silence detection to text insertion
- **Single Binary**: No Python, no runtime dependencies, just one executable
- **Auto-Setup**: Automatic model download with SHA-256 verification

## Quick Start

```bash
# Install
curl -sSL https://voic.sh/install.sh | bash

# Or build from source
git clone https://github.com/burka/voicsh.git
cd voicsh
cargo build --release
cp target/release/voicsh ~/.local/bin/

# First run - downloads model automatically
voicsh setup

# Start the daemon
voicsh start

# Bind a hotkey (e.g., in GNOME Settings → Keyboard → Custom Shortcuts)
# Command: voicsh toggle
# Suggested: Super+Space or Ctrl+Alt+A
```

## Usage

### Basic Voice Typing

1. Press your hotkey to start recording
2. Speak naturally
3. Press hotkey again (or wait for silence detection)
4. Text appears in your focused application

### Commands

```bash
voicsh check              # Check system dependencies
voicsh start              # Start daemon (model loaded, ready for hotkey)
voicsh stop               # Stop daemon
voicsh toggle             # Toggle recording (send to running daemon)
voicsh record             # One-shot: record → transcribe → type → exit
voicsh devices            # List available audio input devices
voicsh models list        # List available models
voicsh models install base.en  # Download a model
voicsh status             # Show daemon status, model info
```

### Configuration

Config file: `~/.config/voicsh/config.toml`

```toml
[audio]
device = "default"           # Audio device (or "pulse", "pipewire", device name)
sample_rate = 16000          # Required for Whisper
vad_threshold = 0.02         # Voice activity detection sensitivity
silence_duration_ms = 1500   # Silence before auto-stop

[stt]
model = "base.en"            # tiny.en, base.en, small.en, medium.en, large
language = "en"              # ISO 639-1 language code

[input]
method = "clipboard"         # "clipboard" (reliable) or "direct" (faster)
paste_key = "ctrl+v"         # Or "ctrl+shift+v" for terminals

[refinement]
enabled = false              # Enable LLM post-processing
provider = "ollama"          # "ollama", "llamacpp", "anthropic", "openai"
model = "gemma2:2b"          # Provider-specific model name
timeout_ms = 3000            # Fallback to raw text if exceeded

[overlay]
enabled = true               # Show recording indicator
position = "top-right"       # top-left, top-right, bottom-left, bottom-right
opacity = 0.8
```

### Environment Variables

All config options can be overridden via environment variables:

```bash
VOICSH_MODEL=small.en voicsh record
VOICSH_REFINEMENT_ENABLED=true voicsh record
VOICSH_AUDIO_DEVICE=alsa_input.usb voicsh start
```

## How It Works

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Hotkey    │───▶│   Record    │───▶│  Transcribe │───▶│    Type     │
│  (trigger)  │    │   (cpal)    │    │ (whisper-rs)│    │  (ydotool)  │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
                          │                  │
                          ▼                  ▼
                   ┌─────────────┐    ┌─────────────┐
                   │     VAD     │    │  Refinement │
                   │  (silence)  │    │   (LLM)     │
                   └─────────────┘    └─────────────┘
```

1. **Hotkey Trigger**: Desktop shortcut sends message to daemon via Unix socket
2. **Audio Capture**: `cpal` records from PipeWire/PulseAudio at 16kHz mono
3. **Voice Activity Detection**: Silence detection auto-stops recording
4. **Transcription**: `whisper-rs` processes audio locally using Whisper models
5. **Refinement** (optional): LLM cleans up punctuation, grammar, formatting
6. **Text Injection**: Clipboard + simulated Ctrl+V via `ydotool` (Wayland) or `xdotool` (X11)

## Requirements

### Runtime Dependencies

**Required for text injection:**
- **Wayland**: `wl-clipboard` and `ydotool` with `ydotoold` daemon running
- **Audio**: PipeWire or PulseAudio (standard on modern Linux)

**Installation:**
```bash
# Check current system dependencies
voicsh check

# Ubuntu/Debian
sudo apt install wl-clipboard ydotool
sudo systemctl enable --now ydotool

# Arch Linux
sudo pacman -S wl-clipboard ydotool
sudo systemctl enable --now ydotool
```

### Build Dependencies

```bash
# Fedora/RHEL
sudo dnf install clang llvm-devel alsa-lib-devel cmake

# Ubuntu/Debian
sudo apt install clang libclang-dev libasound2-dev cmake

# Arch
sudo pacman -S clang alsa-lib cmake
```

## Models

| Model | Size | Speed | Accuracy | Use Case |
|-------|------|-------|----------|----------|
| `tiny.en` | 75 MB | Fastest | Good | Quick notes, low-end hardware |
| `base.en` | 142 MB | Fast | Better | **Recommended for most users** |
| `small.en` | 466 MB | Medium | Great | When accuracy matters |
| `medium.en` | 1.5 GB | Slow | Excellent | Professional transcription |
| `large` | 3 GB | Slowest | Best | Multi-language, highest quality |

English-only models (`.en`) are faster and more accurate for English.

## Wayland Compatibility

Tested and working on:
- GNOME (Wayland)
- KDE Plasma (Wayland)
- Sway
- Hyprland
- wlroots-based compositors

### ydotool Setup (Wayland)

The setup script handles this automatically, but for manual setup:

```bash
# Install ydotool
sudo dnf install ydotool  # Fedora
sudo apt install ydotool  # Ubuntu 23.04+
yay -S ydotool            # Arch AUR

# Enable the daemon
systemctl --user enable --now ydotool

# Add yourself to input group
sudo usermod -aG input $USER
# Log out and back in
```

## Troubleshooting

### No audio captured
```bash
# Check audio devices
voicsh devices

# Test recording
voicsh test-audio

# Try explicit device
voicsh record --device pulse
```

### Text not appearing
```bash
# Check ydotool is running (Wayland)
systemctl --user status ydotool

# Test text injection
voicsh test-input

# Fallback to clipboard-only mode
voicsh record --method clipboard
```

### Model download fails
```bash
# Manual download
voicsh models install base.en --no-verify

# Or download manually and place in cache
wget https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
mv ggml-base.en.bin ~/.cache/voicsh/models/
```

## Comparison with Alternatives

| Feature | voic.sh | nerd-dictation | voxd | BlahST |
|---------|---------|----------------|------|--------|
| Language | Rust | Python | Python | Zsh |
| Single binary | Yes | No | No | No |
| Wayland overlay | Yes | No | No | No |
| Auto model download | Yes | No | Yes | No |
| LLM refinement | Yes | No | Yes | Yes |
| Startup time | <50ms | ~500ms | ~500ms | ~100ms |
| Memory usage | ~50MB | ~200MB | ~300MB | ~100MB |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

MIT License - see [LICENSE](LICENSE)

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Whisper inference engine
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- [smithay-client-toolkit](https://github.com/Smithay/client-toolkit) - Wayland layer-shell
- Inspired by [nerd-dictation](https://github.com/ideasman42/nerd-dictation), [voxd](https://github.com/voxd/voxd), and [BlahST](https://github.com/QuantiusBenignus/BlahST)
