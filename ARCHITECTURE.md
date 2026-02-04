# voic.sh Architecture

Technical architecture of voic.sh, a Rust-based voice typing application for Linux.

## Design Principles

1. **Offline-First**: Core functionality requires no network access
2. **Single Binary**: No runtime dependencies, easy distribution
3. **Subprocess Isolation**: External tools (ydotool, whisper) called as subprocesses for reliability
4. **Progressive Enhancement**: Start simple, add features incrementally
5. **Fail Fast**: Clear error messages, no silent failures

## System Overview

```
┌────────────────────────────────────────────────────────────────────────────┐
│                              voic.sh Daemon                                │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐               │
│  │  IPC Server  │────▶│   Recorder   │────▶│ Transcriber  │               │
│  │ (Unix Socket)│     │   (cpal)     │     │ (whisper-rs) │               │
│  └──────────────┘     └──────────────┘     └──────────────┘               │
│         │                    │                    │                        │
│         │                    ▼                    ▼                        │
│         │             ┌──────────────┐     ┌──────────────┐               │
│         │             │     VAD      │     │  Refinement  │               │
│         │             │  (threshold) │     │ (LLM - opt)  │               │
│         │             └──────────────┘     └──────────────┘               │
│         │                                        │                        │
│         │                                        ▼                        │
│         │                               ┌──────────────┐                  │
│         │                               │   Injector   │                  │
│         │                               │  (ydotool)   │                  │
│         │                               └──────────────┘                  │
│         │                                        │                        │
│         ▼                                        ▼                        │
│  ┌──────────────┐                       ┌──────────────┐                  │
│  │   Overlay    │                       │   Clipboard  │                  │
│  │ (layer-shell)│                       │  (wl-copy)   │                  │
│  └──────────────┘                       └──────────────┘                  │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘

External:
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Hotkey     │────▶│  voicsh CLI  │────▶│ Unix Socket  │
│  (Desktop)   │     │   (toggle)   │     │   Message    │
└──────────────┘     └──────────────┘     └──────────────┘
```

## Components

### 1. IPC Server

**Purpose**: Receive commands from CLI, coordinate recording state

**Location**: `src/ipc/server.rs`

- Unix socket at `~/.local/share/voicsh/voicsh.sock`
- Simple line-based JSON protocol
- Commands: toggle, start, stop, cancel, status, shutdown

**Why Unix Socket**:
- No network exposure (security)
- Fast IPC (~1ms latency)
- Standard Linux pattern
- Works without D-Bus dependency

### 2. Audio Recorder

**Purpose**: Capture microphone input at 16kHz mono

**Location**: `src/audio/recorder.rs`

**Audio Format** (Whisper requirement):
- Sample rate: 16,000 Hz
- Channels: 1 (mono)
- Format: i16 PCM (signed 16-bit)
- Buffer: In-memory, typically 100KB-1MB per utterance

**Device Selection Priority**:
1. Explicit device from config
2. `VOICSH_AUDIO_DEVICE` environment variable
3. PipeWire/PulseAudio default device
4. ALSA default device

**Crate**: `cpal` for cross-platform audio capture

### 3. Voice Activity Detection (VAD)

**Purpose**: Detect speech start/end for automatic recording control

**Location**: `src/audio/vad.rs`

**Configuration**:
- `speech_threshold`: RMS threshold to detect speech (default: 0.02)
- `silence_duration_ms`: Duration of silence before stopping (default: 1500)
- `min_speech_ms`: Minimum speech duration before allowing stop (default: 300)

**VAD Approaches** (in order of complexity):
1. **RMS Threshold** (MVP): Simple, effective for quiet environments
2. **WebRTC VAD**: `webrtc-vad` crate, better noise handling
3. **Silero VAD**: ONNX model, best accuracy, higher CPU

### 4. Transcriber (STT)

**Purpose**: Convert audio to text using Whisper

**Location**: `src/stt/whisper.rs`

**Crate**: `whisper-rs` (bindings to whisper.cpp)

**Model Loading Strategy**:
- **Cold Start**: Load model on first transcription (~1-3s for base.en)
- **Warm (Daemon)**: Keep model loaded in memory (~300MB for base.en)
- **GPU Acceleration**: Optional CUDA/Metal support via whisper-rs features

**Alternative**: HTTP to whisper.cpp server for lowest latency when model is large

### 5. Text Injector

**Purpose**: Insert transcribed text into the focused application

**Location**: `src/input/injector.rs`

**Input Methods**:
- **Clipboard** (default): Copy to clipboard, simulate Ctrl+V - more reliable
- **Direct**: Type characters directly - faster but less reliable with special chars

**Session Detection**: Checks `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`

**External Tools**:
- Wayland: `wl-copy` + `ydotool`
- X11: `xsel` + `xdotool`

**Why Clipboard Method is Default**:
- More reliable character encoding (UTF-8 handled correctly)
- Works with all applications
- No issues with special characters

### 6. LLM Refinement (Optional)

**Purpose**: Clean up transcription with AI post-processing

**Location**: `src/refinement/mod.rs`

**Providers**:
- None (passthrough)
- Ollama (local)
- llama.cpp server (local)
- Anthropic Claude API (cloud)
- OpenAI API (cloud)

**Behavior**:
- Async with configurable timeout
- Falls back to raw transcription on error/timeout
- Default prompt: punctuation/grammar cleanup

### 7. Overlay (Wayland Layer-Shell)

**Purpose**: Visual feedback during recording

**Location**: `src/overlay/mod.rs`

**States**:
- Hidden
- Recording (red indicator)
- Transcribing (orange indicator)
- Error (bright red)

**Implementation**: `smithay-client-toolkit` for Wayland layer-shell protocol

**Fallback for X11**: Desktop notification or GTK overlay

### 8. Model Manager

**Purpose**: Download and manage Whisper models

**Location**: `src/models/manager.rs`

**Features**:
- Auto-download from HuggingFace
- SHA-256 verification
- Progress bar during download
- Model catalog with size/accuracy metadata

**Cache Location**: `~/.cache/voicsh/models/`

### 9. Configuration

**Purpose**: User settings management

**Location**: `src/config.rs`

**Format**: TOML

**Sections**: audio, stt, input, refinement, overlay

**Environment Overrides**: `VOICSH_*` variables override config file

## Directory Structure

```
~/.config/voicsh/
└── config.toml              # User configuration

~/.cache/voicsh/
└── models/
    ├── ggml-tiny.en.bin
    ├── ggml-base.en.bin
    └── ggml-small.en.bin

~/.local/share/voicsh/
├── voicsh.sock              # IPC socket (daemon)
└── voicsh.log               # Optional debug log
```

## Data Flow

### Recording Flow

1. User presses hotkey
2. Desktop runs: `voicsh toggle`
3. CLI connects to Unix socket
4. CLI sends toggle command
5. Daemon starts audio recording (cpal)
6. Daemon shows overlay → Recording state
7. Audio samples flow to VAD
8. VAD detects silence (1.5s)
9. Daemon stops recording
10. Daemon shows overlay → Transcribing state
11. Audio sent to whisper-rs
12. Whisper returns text
13. (Optional) Text sent to LLM for refinement
14. Text copied to clipboard (wl-copy)
15. Paste keystroke simulated (ydotool)
16. Overlay hidden
17. Text appears in focused application

### Latency Budget

**Target**: < 500ms from silence detection to text insertion

| Step | Time |
|------|------|
| Silence detection | 0ms (already detected) |
| Audio finalization | 10ms |
| Whisper inference | 200-400ms (base.en) |
| Clipboard copy | 10ms |
| Keystroke simulation | 20ms |
| Application paste | 50ms |
| **Total** | **290-490ms** |

(Add 0-2000ms if LLM refinement enabled)

## Error Handling

### Graceful Degradation

| Error | Fallback Chain |
|-------|----------------|
| Audio device not found | Try fallback devices → Error with suggestion |
| ydotool not running | Start ydotool → Clipboard-only mode |
| Model not found | Auto-download → Manual instructions |
| LLM timeout | Use raw transcription |
| Wayland not detected | Fall back to X11 → Error if neither |

### User Feedback

- Overlay shows state changes
- CLI commands return clear error messages
- `voicsh status` shows full system health
- Debug mode logs to file

## Security Considerations

1. **No Network by Default**: Core functionality is offline
2. **Unix Socket Permissions**: Socket created with user-only permissions (0600)
3. **No Root Required**: Runs entirely in userspace
4. **Audio Privacy**: Audio processed locally, never stored permanently
5. **LLM API Keys**: Environment variables or user-readable config file

## Technology Choices

### Why These Tools?

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Audio | cpal | Cross-platform, PipeWire/Pulse/ALSA support |
| STT | whisper-rs | Best accuracy, active development, Rust bindings |
| Overlay | smithay-client-toolkit | Native Wayland layer-shell |
| Config | TOML + serde | Rust idiomatic, human-readable |
| CLI | clap | Best-in-class arg parsing |
| Async | tokio | Industry standard |
| IPC | Unix socket | Simple, secure, fast |

### Alternatives Considered

| Alternative | Why Not |
|-------------|---------|
| D-Bus instead of Unix socket | Heavier dependency |
| Embedding model in binary | Too large, harder to update |
| Direct uinput for text injection | Requires root/capabilities |
| GTK/Qt for overlay | Heavier, but more portable |
| VOSK instead of Whisper | Lower accuracy for English |

## Future Considerations

### Potential Optimizations
- GPU acceleration for Whisper (CUDA/Metal)
- Streaming transcription (word-by-word)
- Voice command detection
- Multiple language support
- Custom wake words

### Technical Debt to Avoid
- Don't embed large models
- Don't require root privileges
- Don't bypass clipboard for special apps
- Don't add GUI settings (config file is enough)
