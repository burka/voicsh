# voicsh Architecture

Wayland-only voice typing: audio capture → VAD → chunking → Whisper transcription → text injection.

## Design Principles

1. **Offline-First** — No network for core functionality
2. **Single Binary** — No runtime interpreters
3. **Fail Fast** — Clear errors, no silent failures
4. **Pluggable** — Traits for audio source, transcriber, text sink

## Pipeline

```
AudioSource → VadStation → ChunkerStation → TranscriberStation → SinkStation
    │              │              │                  │                 │
    └──────────────┴──────────────┴──────────────────┴─────────────────┘
                        All connected via crossbeam::channel::bounded
```

Each station implements `Station<Input, Output>` and runs in its own thread via `StationRunner`.

### Stations

| Station | Input | Output | Purpose |
|---------|-------|--------|---------|
| VadStation | AudioFrame | VadFrame | RMS-based speech detection, level meter (`-v`) |
| ChunkerStation | VadFrame | AudioChunk | Gap-shrinking adaptive chunker |
| TranscriberStation | AudioChunk | TranscribedText | whisper-rs inference |
| SinkStation | TranscribedText | () | Delegates to TextSink impl |

### TextSink Implementations

- **InjectorSink** — Clipboard + paste key simulation (continuous mode)
- **CollectorSink** — Accumulates text, returns on finish (`--once` mode)
- **StdoutSink** — Writes to stdout (pipe mode: `cat file.wav | voicsh`)

### Text Injection Fallback Chain

1. **Portal** — xdg-desktop-portal RemoteDesktop key injection (GNOME 45+, KDE 6.1+)
2. **wtype** — wlroots virtual keyboard
3. **ydotool** — uinput-based, works everywhere but needs daemon

Paste key auto-detection: queries swaymsg → hyprctl → GNOME Shell Introspect → GNOME fallback → generic fallback.

## Module Map

```
src/
├── app.rs                  # Orchestrates record command (feature-gated: cpal-audio+model-download+cli)
├── cli.rs                  # clap argument parsing (-v/-vv, --once, --fan-out, etc.)
├── config.rs               # TOML config + env overrides
├── diagnostics.rs          # `voicsh check` dependency validation
├── audio/
│   ├── capture.rs          # cpal AudioSource impl (feature: cpal-audio)
│   ├── recorder.rs         # AudioSource trait + MockAudioSource
│   ├── wav.rs              # WavAudioSource: WAV file input with resampling (hound)
│   └── vad.rs              # Voice activity detection (RMS threshold + state machine)
├── input/
│   ├── injector.rs         # TextInjector with CommandExecutor trait (wl-copy, wtype, ydotool)
│   ├── portal.rs           # ashpd PortalSession for key injection (feature: portal)
│   └── focused_window.rs   # Paste key detection (sway/hyprland/GNOME)
├── pipeline/
│   ├── orchestrator.rs     # Pipeline + PipelineConfig + PipelineHandle
│   ├── station.rs          # Station trait + StationRunner
│   ├── sink.rs             # TextSink trait, SinkStation, InjectorSink, CollectorSink, StdoutSink
│   ├── adaptive_chunker.rs # Gap-shrinking chunker algorithm
│   ├── vad_station.rs      # VAD as Station
│   ├── chunker_station.rs  # Chunker as Station
│   ├── transcriber_station.rs # Transcriber as Station
│   ├── types.rs            # AudioFrame, VadFrame, AudioChunk, TranscribedText
│   └── error.rs            # StationError + ErrorReporter trait
├── streaming/              # Experimental streaming pipeline (ring buffer, stitcher)
├── stt/
│   ├── transcriber.rs      # Transcriber trait + MockTranscriber
│   ├── whisper.rs          # WhisperTranscriber (feature: whisper)
│   └── fan_out.rs          # Parallel model comparison (--fan-out)
├── models/
│   ├── catalog.rs          # Model metadata, English/multilingual variants
│   └── download.rs         # HuggingFace download with SHA-1 verification (feature: model-download)
└── ipc/
    ├── protocol.rs         # JSON command/response types
    └── server.rs           # Unix socket IPC server
```

## Feature Gates

```toml
default = ["full"]
full    = ["whisper", "cpal-audio", "model-download", "cli", "portal"]
cuda    = ["whisper-rs/cuda"]       # NVIDIA GPU
vulkan  = ["whisper-rs/vulkan"]     # Cross-platform GPU
hipblas = ["whisper-rs/hipblas"]    # AMD GPU
openblas = ["whisper-rs/openblas"]  # CPU BLAS
```

Use `--no-default-features` for fast lib-only builds (skips whisper-rs compilation).

## Verbosity

- No flag: text only (`"transcribed text"`)
- `-v`: volume meter + result lines (`[ok 43ch] "text"`)
- `-vv`: full diagnostics (chunk timing, transcribing progress, paste detection steps)

## Configuration

TOML at `~/.config/voicsh/config.toml`. Sections: `[audio]`, `[stt]`, `[input]`.
Environment overrides: `VOICSH_MODEL`, `VOICSH_AUDIO_DEVICE`, etc.
