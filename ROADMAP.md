---
title: Roadmap
weight: 7
---

## 0.0.1 — First release (done)

Default command, pipe mode, GPU, daemon mode.

- [x] Default command: `voicsh` alone starts mic recording (no subcommand)
- [x] Pipe mode: `cat file.wav | voicsh` → transcribe → stdout
- [x] Auto-resample WAV to 16kHz mono (linear interpolation)
- [x] GPU feature gates: `--features cuda`, `vulkan`, `hipblas`
- [x] Logging cleanup: all output respects -v/-vv levels consistently
- [x] Honest README matching actual features
- [x] Unix socket IPC: `voicsh start` / `voicsh stop` / `voicsh toggle`
- [x] Model stays in memory (~300MB for base.en)
- [x] Systemd user service: `voicsh install-service`
- [x] `voicsh status` shows daemon health
- [x] Shell completions (bash/zsh/fish)
- [x] `voicsh init` — auto-tune: benchmark hardware, recommend model, download
- [x] `voicsh follow` — stream daemon events (meter, state, transcriptions)
- [x] `voicsh config get/set/list/dump` — manage config without a text editor
- [x] Hallucination filtering (configurable, language-specific)
- [x] Fan-out mode — run English + multilingual models in parallel, pick best

## 0.1.0 — Voice commands (in progress)

Spoken punctuation, formatting, and keyboard control.

- [x] Punctuation: "period", "comma", "question mark", "exclamation mark", etc.
- [x] Whitespace: "new line", "new paragraph", "space", "tab"
- [x] Caps toggle: "all caps" / "end caps"
- [x] Symbols: slash, ampersand, at-sign, dollar, hash, percent, asterisk, etc.
- [x] Brackets: open/close paren, brace, bracket, angle bracket
- [x] Key combos: "delete word" (Ctrl+Backspace), "backspace"
- [x] Configurable vocabulary in config.toml (user overrides merge with built-ins)
- [x] Rule-based (no LLM needed)
- [x] Multi-language: en, de, es, fr, pt, it, nl, pl, ru, ja, zh, ko
- [x] GNOME Shell extension: install, toggle, status indicator (`voicsh install-gnome-extension`)
- [x] Language allowlist: filter transcriptions by allowed languages + confidence threshold
- [x] GNOME extension: "Open Debug Log" menu item (launches `voicsh follow` in terminal)
- [ ] Voice commands working reliably end-to-end
- [ ] GNOME Shell extension: model switcher, language picker

## 0.2.0 — Post-ASR error correction

FlanEC (flan-t5-base) via candle for post-ASR error correction.

- Single-best hypothesis + per-word confidence markup as input
- Lazy-loaded ~250M param model, F16 (~500MB), timeout + fallback to raw text
- English-only guard (passthrough for other languages)
- `[error_correction]` config section

## 0.3.0 — GPU and improved correction

GPU acceleration and enhanced error correction pipeline.

- **Research task:** Evaluate candle Whisper — if viable, migrate from whisper.cpp for unified GPU context + N-best hypotheses (beam search → FlanEC gets 5-best input as trained)
- **Alternative if candle Whisper doesn't work out:** Keep whisper.cpp, add multi-pass correction with optional smaller models to stay real-time
- CUDA already partially working on dev machine
- Unified `candle-core/cuda` feature covers both Whisper + FlanEC (if candle path chosen)
- GPU compilation gates: CUDA, Vulkan, hipBLAS in CI containers
- Vulkan runtime tests via lavapipe

## 0.4.0 — Overlay and sentence refinement

Wayland overlay that collects dictated fragments, refines them with an LLM into complete sentences, then injects the polished result.

- Wayland layer-shell overlay: recording indicator + live transcription display
- Sentence collector: buffer dictated chunks in the overlay instead of injecting immediately
- LLM sentence stitcher: when a sentence is complete (detected by LLM), refine and inject
  - Context-aware: knows the previous sentence for coherent flow
  - Uses local LLM (Ollama / llama.cpp) or cloud (Anthropic, OpenAI)
  - Timeout + fallback to raw transcription if LLM is unavailable

## 0.5.0 — LLM assistant

Voice-activated LLM: hold key + speak a question → LLM processes → answer injected as text.

- Local: Ollama, llama.cpp server (auto-detect running)
- Cloud providers optional (Anthropic, OpenAI)
- Timeout + fallback

## Future

- Streaming word-by-word display
- Push-to-talk (hold hotkey)
- X11 support (xdotool/xsel)
- Profiles (per-app settings)
- Daemon: listen for PipeWire/PulseAudio device changes, auto-recover or show helpful message
- German grammar correction: t5-small-grammar-correction-german (aiassociates) via candle
- Deepgram remote API integration (cloud ASR alternative)
- NVIDIA Canary / NeMo support via local docker container (nvcr.io/nvidia/nemo) — high-quality local ASR alternative (~20GB+)

## Non-goals

- GUI settings app (config file is enough)
- Speaker identification
- Real-time translation
- Windows/macOS (Linux-first)
