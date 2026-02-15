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

## 0.1.0 — Usable voice typing (in progress)

Multi-language support, GNOME Shell extension, spoken punctuation, per-token confidence, hallucination filtering, and quantized models.

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
- [x] GNOME extension: follow mode with live audio levels, recording state, transcriptions
- [x] GNOME extension: language picker + model switcher via IPC (SetLanguage/SetModel)
- [x] GNOME extension: language indicator in panel (two-letter code next to icon, configurable)
- [x] Unified output: daemon verbose and `voicsh follow` share one renderer (DRY)
- [x] Per-token confidence coloring: real decoder probabilities (green/default/yellow/red)
- [x] Hallucination filter: 76+ phrases, CJK punctuation normalization, punctuation-only skip
- [x] Quantized model support (q5_0, q5_1, q8_0 variants)

## 0.2.0 — Post-ASR error correction

LLM-based error correction using per-token confidence as a guide.

- Per-token probability data already available (`TokenProbability { token, probability }`)
- Low-probability tokens are flagged as correction candidates for the LLM
- **Option A — Local model:** FlanEC (flan-t5-base) or instruction-tuned LLM via candle (~250M–1B params, F16)
- **Option B — External LLM:** Ollama / llama.cpp / cloud API with token-confidence prompt
- Lazy-loaded model, timeout + fallback to raw transcription
- Greedy reranking: bump `best_of: 1` → `best_of: 5` for better base accuracy before correction
- English-only guard initially (passthrough for other languages)
- `[error_correction]` config section

## 0.3.0 — GPU and improved correction

GPU acceleration and enhanced error correction pipeline.

- **Research task:** Evaluate candle Whisper — if viable, migrate from whisper.cpp for unified GPU context
- **N-best reality:** whisper-rs/whisper.cpp BeamSearch returns single-best only (no n-best extraction). True n-best requires either candle Whisper or multi-pass with different temperatures.
- **Alternative path:** Keep whisper.cpp + BeamSearch for better single-best, feed per-token probabilities to LLM corrector (already wired)
- CUDA already partially working on dev machine
- Unified `candle-core/cuda` feature covers both Whisper + correction model (if candle path chosen)
- GPU compilation gates: CUDA, Vulkan, hipBLAS in CI containers
- Vulkan runtime tests via lavapipe

## 0.4.0 — Reliable spoken punctuation and overlay

Spoken punctuation that works reliably. Wayland overlay for live feedback.

- Reliable spoken punctuation end-to-end (building on 0.1.0 foundation + 0.2.0 error correction)
- Wayland layer-shell overlay: recording indicator + live transcription display
- Per-token confidence visualization in overlay (color-coded, same scale as terminal)
- Sentence collector: buffer dictated chunks in the overlay instead of injecting immediately
- LLM sentence stitcher: when a sentence is complete (detected by LLM), refine and inject
  - Receives token-level probabilities to focus corrections on uncertain tokens
  - Context-aware: knows the previous sentence for coherent flow
  - Uses local LLM (Ollama / llama.cpp) or cloud (Anthropic, OpenAI)
  - Timeout + fallback to raw transcription if LLM is unavailable

## 0.5.0 — Voice commands

Full voice commands beyond punctuation — navigation, selection, editing, app control.

- Voice commands working reliably end-to-end (leveraging 0.4.0 reliable punctuation pipeline)
- Navigation: "go to line", "scroll up/down", "page up/down"
- Selection: "select word", "select line", "select all"
- Editing: "undo", "redo", "copy", "paste", "cut"
- Extensible command vocabulary via config

## 0.6.0 — LLM assistant

Voice-activated LLM: hold key + speak a question → LLM processes → answer injected as text.

- Local: Ollama, llama.cpp server (auto-detect running)
- Cloud providers optional (Anthropic, OpenAI)
- Timeout + fallback

## Future

- Streaming token-by-token display (live partial results during recording)
- Push-to-talk (hold hotkey)
- X11 support (xdotool/xsel)
- Profiles (per-app settings)
- Daemon: listen for PipeWire/PulseAudio device changes, auto-recover or show helpful message
- German grammar correction: t5-small-grammar-correction-german (aiassociates) via candle
- Deepgram remote API integration (cloud ASR alternative)
- NVIDIA Canary / NeMo support via local docker container (nvcr.io/nvidia/nemo) — high-quality local ASR alternative (~20GB+)
- GNOME extension: portal per-recording (close RemoteDesktop session when idle, remove yellow privacy indicator)

## Non-goals

- GUI settings app (config file is enough)
- Speaker identification
- Real-time translation
- Windows/macOS (Linux-first)
