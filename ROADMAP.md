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

## 0.2.0 — Text refinement

LLM post-processing for polished output.

- Local: Ollama, llama.cpp server (auto-detect running)
- `--refine default|formal|casual|code`
- Timeout + fallback to raw transcription
- Cloud providers optional (Anthropic, OpenAI)

## 0.3.0 — GPU and extension testing

Container-based testing for GPU backends and GNOME integration.

- GPU compilation gates: verify `--features cuda/vulkan/hipblas` build in vendor containers
  - CUDA: `nvidia/cuda:12.6.1-devel-ubuntu24.04` (compile-only, no GPU needed)
  - hipblas: `rocm/dev-ubuntu-24.04:6.4-complete` (compile-only, no GPU needed)
  - Vulkan: `libvulkan-dev` + `mesa-vulkan-drivers` on Ubuntu (compile **and** run)
- Vulkan runtime tests via lavapipe (Mesa software Vulkan, `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`)
- GNOME extension integration tests via `ghcr.io/ddterm/gnome-shell-image/fedora-43` (or other distros)
  - Headless: `gnome-shell --wayland --headless --unsafe-mode --virtual-monitor 1600x960`
  - Install/enable via `gnome-extensions` CLI, check `journalctl` for errors
  - Screenshots via D-Bus `org.gnome.Shell.Screenshot`
- CI workflow for all of the above on GitHub Actions (standard runners, no GPU/KVM needed)

## 0.4.0 — Overlay and sentence refinement

Wayland overlay that collects dictated fragments, refines them with an LLM into complete sentences, then injects the polished result.

- Wayland layer-shell overlay: recording indicator + live transcription display
- Sentence collector: buffer dictated chunks in the overlay instead of injecting immediately
- LLM sentence stitcher: when a sentence is complete (detected by LLM), refine and inject
  - Context-aware: knows the previous sentence for coherent flow
  - Uses local LLM (Ollama / llama.cpp) or cloud (Anthropic, OpenAI)
  - Timeout + fallback to raw transcription if LLM is unavailable

## Future

- Streaming word-by-word display
- Push-to-talk (hold hotkey)
- X11 support (xdotool/xsel)
- Profiles (per-app settings)
- Daemon: listen for PipeWire/PulseAudio device changes, auto-recover or show helpful message

## Non-goals

- GUI settings app (config file is enough)
- Speaker identification
- Real-time translation
- Windows/macOS (Linux-first)
