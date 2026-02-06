# voicsh Roadmap

## 0.0.1 — First release

Default command, pipe mode, GPU, daemon mode.

- Default command: `voicsh` alone starts mic recording (no subcommand)
- Pipe mode: `cat file.wav | voicsh` → transcribe → stdout
- Auto-resample WAV to 16kHz mono (linear interpolation)
- GPU feature gates: `--features cuda`, `vulkan`, `hipblas`
- Logging cleanup: all output respects -v/-vv levels consistently
- Honest README matching actual features
- Unix socket IPC: `voicsh start` / `voicsh stop` / `voicsh toggle`
- Model stays in memory (~300MB for base.en)
- Systemd user service: `voicsh install-service`
- `voicsh status` shows daemon health

## 0.1.0 — Voice commands

Spoken punctuation and formatting.

- "new line", "new paragraph", "period", "comma", "question mark"
- "all caps" / "end caps" toggle
- Configurable vocabulary in config.toml
- Rule-based (no LLM needed)

## 0.2.0 — Text refinement

LLM post-processing for polished output.

- Local: Ollama, llama.cpp server (auto-detect running)
- `--refine default|formal|casual|code`
- Timeout + fallback to raw transcription
- Cloud providers optional (Anthropic, OpenAI)

## Future

- Overlay feedback (Wayland layer-shell recording indicator)
- Streaming word-by-word display
- Push-to-talk (hold hotkey)
- X11 support (xdotool/xsel)
- Profiles (per-app settings)
- Benchmarking (`voicsh benchmark`)

## Non-goals

- GUI settings app (config file is enough)
- Speaker identification
- Real-time translation
- Windows/macOS (Linux-first)
