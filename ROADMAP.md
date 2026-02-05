# voic.sh Development Roadmap

Phased development plan from MVP to full-featured voice typing application.

## Project Phases Overview

| Phase | Focus | Status |
|-------|-------|--------|
| **MVP** | Core voice loop | Done |
| **Phase 2** | Essential UX | In Progress |
| **Phase 3** | Text Intelligence | Planned |
| **Phase 4** | Advanced Features | Future |

---

## MVP - Core Voice Loop

**Goal**: Record → Transcribe → Type (working end-to-end)

### Deliverables

- [x] **Project scaffold**: Cargo.toml, module structure, CI/CD
- [x] **Audio capture**: cpal-based recording at 16kHz mono, device selection
- [x] **Voice Activity Detection**: RMS threshold, configurable silence duration, manual stop
- [x] **STT integration**: whisper-rs, hardcoded model path, basic post-processing
- [x] **Text injection**: Wayland (wl-copy + ydotool), X11 (xsel + xdotool), auto-detection
- [x] **CLI**: `voicsh record`, `voicsh devices`, `--version`/`--help`
- [x] **Configuration**: TOML config, environment overrides, sensible defaults

### Success Criteria

- [x] `voicsh record` captures audio and outputs transcription
- [x] Text appears in focused application after recording
- [x] Works on Wayland (GNOME, KDE, Sway)
- [x] Latency < 500ms from silence detection to text
- [x] No crashes during 30-minute session
- [x] Base.en model produces >90% accuracy for clear speech

---

## Phase 2 - Essential UX

**Goal**: Make it actually usable day-to-day

### Model Management

- [x] Auto-download from HuggingFace with progress bar
- [x] SHA-256 verification, retry on failure
- [x] Model catalog: tiny.en, base.en, small.en, medium.en, large + quantized
- [x] CLI: `voicsh models list/install/remove/use`
- [x] XDG-compliant cache at `~/.cache/voicsh/models/`

### Daemon Mode

- [ ] Unix socket IPC server for instant response
- [ ] Keep model loaded in memory
- [ ] CLI: `voicsh start/stop/toggle/status`
- [ ] Systemd user service integration

### Overlay Feedback

- [ ] Wayland layer-shell overlay (smithay-client-toolkit)
- [ ] Recording/transcribing/error indicators
- [ ] Configurable position and opacity
- [ ] X11 fallback (notification or GTK overlay)

### Audio Pipeline Improvements

- [ ] webrtc-vad integration option
- [ ] Device listing with details
- [ ] Audio diagnostics: `voicsh test-audio`, `voicsh test-input`

### System Integration

- [ ] Setup wizard: dependency check, model download, config creation
- [ ] XDG compliance for all paths

### Success Criteria

- [ ] First run downloads model automatically
- [ ] `voicsh setup` gets new users working in < 2 minutes
- [ ] Daemon starts in < 100ms, transcription < 300ms
- [ ] Overlay visible on GNOME, KDE, Sway, Hyprland
- [ ] Works after reboot (systemd autostart)

---

## Phase 3 - Text Intelligence

**Goal**: AI-powered refinement for professional output

### Post-Processing Pipeline

- [ ] Built-in cleanup: auto-capitalization, punctuation, spacing, number formatting
- [ ] Configurable rules: enable/disable processors, custom replacements, regex

### LLM Refinement

- [ ] Provider abstraction with timeout/fallback
- [ ] Local: Ollama, llama.cpp server (auto-detect running servers)
- [ ] Cloud: Anthropic Claude, OpenAI (env var API keys)
- [ ] Prompt presets: default, formal, casual, technical, code

### Voice Commands

- [ ] Basic: "new line", "new paragraph", punctuation, "all caps"/"end caps"
- [ ] Configurable vocabulary in TOML

### Success Criteria

- [ ] Built-in cleanup improves readability without LLM
- [ ] LLM refinement < 2s additional latency
- [ ] Graceful fallback when LLM unavailable
- [ ] Voice commands work reliably

---

## Phase 4 - Advanced Features

**Goal**: Power user capabilities

### Input Modes

- [ ] Push-to-talk: hold hotkey to record, release to transcribe
- [ ] Continuous dictation: incremental insertion, "stop dictation" command
- [ ] Wake word activation (optional, privacy documented)

### Wayland Enhancements

- [ ] Virtual keyboard protocol (zwp_virtual_keyboard_v1) - no ydotool
- [ ] Primary selection support, preserve clipboard option
- [ ] Multi-monitor overlay positioning

### Streaming Pipeline Architecture (Priority)

**Goal**: Low-latency continuous transcription with chunked processing

**Status**: Core continuous pipeline with station-based architecture is implemented in `src/pipeline/`. Real-time word-by-word display, whisper.cpp WebSocket, and live correction remain future work.

```
┌─────────────┐    ┌─────────────┐    ┌──────────┐    ┌───────────┐    ┌─────────┐
│  Continuous │───▶│  Silence    │───▶│ Chunker  │───▶│Transcriber│───▶│ Stitcher│───▶ Inject
│  Recording  │    │  Detector   │    │          │    │  (async)  │    │         │
└─────────────┘    └─────────────┘    └──────────┘    └───────────┘    └─────────┘
       │                  │                 ▲
       ▼                  │                 │
   Ring Buffer            └── control ──────┘
                             (flush chunk
                              on silence)
```

- [x] **Ring buffer audio capture**
  - Continuous recording to circular buffer
  - Never stops until user ends session
  - Decoupled from transcription timing

- [x] **Silence detector (separate station)**
  - Monitors audio stream for pauses
  - Sends control frames to chunker:
    - `FlushChunk` - silence detected, process current buffer immediately
    - `SpeechStart` - speech resumed after silence
  - Configurable silence threshold and duration

- [x] **Chunker**
  - Receives audio data + control frames
  - Emits chunks on:
    - Time threshold reached (~3s default)
    - `FlushChunk` control frame received
  - `--chunk-size=N` / `-s N` CLI override
  - Small overlap between chunks for word continuity (~200ms)

- [x] **Async transcription pipeline**
  - Process chunks as they arrive
  - Don't block recording while transcribing
  - Queue management for slow transcription

- [ ] **Result stitching**
  - Combine chunk transcriptions seamlessly
  - Handle word boundaries (avoid duplicates/cuts)
  - Punctuation continuity

- [x] **Auto-leveling / AGC** (after chunking works)
  - Automatic gain control for varying input volumes
  - Normalize audio before transcription
  - Adaptive threshold based on ambient noise

### Streaming STT (Future)

- [ ] Real-time word-by-word display
- [ ] whisper.cpp server WebSocket integration
- [ ] Live correction as you speak

### Advanced Configuration

- [ ] Profiles: work, casual, code with different models/prompts
- [ ] Per-application settings and auto-profile selection
- [ ] Custom vocabulary and Whisper prompt engineering

### Performance & Debugging

- [ ] GPU acceleration (CUDA, Metal)
  - Build whisper.cpp with CUDA support
  - Auto-detect GPU availability
  - Fallback to CPU when GPU unavailable
  - Config option: `stt.use_gpu = auto|always|never`
- [ ] Latency tracking and accuracy estimation
- [ ] Debug mode: save audio, log transcriptions, profiling

### Model Auto-Selection & Benchmarking

- [ ] `voicsh benchmark` command
  - Measure transcription speed for each installed model
  - Report accuracy estimate (optional test audio)
  - Recommend optimal model for current hardware
- [ ] Auto-select model based on system resources
  - Detect available RAM and CPU cores
  - Heuristic: <4GB → tiny, 4-8GB → base, 8-16GB → small, >16GB → medium
  - Config option: `stt.model = auto` to enable
- [ ] Resource usage limits
  - Config: `stt.max_memory_mb` - limit model memory
  - Config: `stt.max_cpu_percent` - limit CPU usage during transcription
  - Graceful degradation to smaller model if limits exceeded

### Success Criteria

- [ ] Streaming mode shows words as spoken
- [ ] Push-to-talk feels instant
- [ ] Profiles switch seamlessly
- [ ] GPU acceleration 2x faster than CPU
- [ ] `voicsh benchmark` recommends appropriate model
- [ ] Auto-selected model works well on low-end hardware (4GB RAM)

---

## Technical Debt & Quality

### Ongoing

- [ ] Unit tests (>80% coverage)
- [ ] Integration tests for full pipeline
- [ ] Fuzz testing for audio/text processing
- [ ] Benchmarks for critical paths
- [ ] Documentation (rustdoc, mdbook)

### Code Quality Gates

- `cargo fmt` - consistent formatting
- `cargo clippy` - lint clean
- `cargo test` - all passing
- `cargo audit` - no vulnerabilities
- `cargo deny` - license compliance

---

## Distribution

### Packaging

- [ ] Static binary (musl) - single file download
- [ ] Cargo: `cargo install voicsh`
- [ ] Arch AUR, Fedora COPR, Ubuntu PPA, Nix flake
- [ ] Flatpak (maybe - sandboxing challenges)

### Documentation

- [ ] README with quick start
- [ ] ARCHITECTURE.md
- [ ] User guide (mdbook)
- [ ] API docs (rustdoc)
- [ ] Video tutorial

---

## Non-Goals (Explicit Scope Limits)

- **Voice commands for system control** - Focus on dictation only
- **Speaker identification** - Privacy concern, complexity
- **Real-time translation** - Different use case
- **Mobile support** - Linux desktop only
- **GUI settings application** - Config file is sufficient
- **Windows/macOS support** - Linux-first (PRs welcome)
- **Browser extension** - System-wide approach instead
- **Electron/web UI** - Native Rust only

---

## Inspiration & Prior Art

| Project | What We Learned |
|---------|-----------------|
| **nerd-dictation** | Simple architecture, VOSK streaming, multiple input tools |
| **voxd** | Model management, multi-provider LLM, daemon pattern |
| **BlahST** | sox VAD, whisper.cpp server, LLM wake words |
| **whisper.cpp** | GGML models, server mode, performance tuning |

---

## Changelog

### Unreleased
- Unified pipeline architecture (src/pipeline/) with pluggable TextSink
- Feature-gated dependencies (cpal-audio, whisper, model-download, cli)
- InjectorSink for voice typing, CollectorSink for text collection
- Pipeline API: Pipeline::start(source, transcriber, sink) → PipelineHandle
- --once mode now uses same pipeline with CollectorSink
- Lib-only build support with --no-default-features
