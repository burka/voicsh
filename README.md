---
title: "voic.sh \u2014 Voice typing for Wayland Linux"
linkTitle: Overview
cascade:
  type: docs
---
[![crates.io](https://img.shields.io/crates/v/voicsh.svg)](https://crates.io/crates/voicsh)
[![docs](https://img.shields.io/badge/docs-voic.sh-blue)](https://voic.sh)

Offline, privacy-first voice typing. Speak into your mic, text appears in your focused app. Or pipe a WAV file and get text on stdout.

**Build:** Rust, C compiler, cmake, pkg-config, libclang, ALSA headers — `sudo apt install build-essential cmake pkg-config libclang-dev libasound2-dev`

**Run (mic mode):** `sudo apt install wl-clipboard wtype ydotool` — GNOME 45+ / KDE 6.1+ work without wtype/ydotool; pipe mode has no runtime deps

> **Status: Early MVP (v0.0.1)**
>
> Free-time side project. Primary target: **Ubuntu + GNOME + Wayland** — that's what I develop and test on. Other distros, desktops, and compositors are welcome, but I can't reproduce issues outside this setup. Maintenance time is limited.
>
> If something doesn't work, [open an issue](https://github.com/burka/voicsh/issues) so we can improve it together. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to make the most of limited maintenance bandwidth.

## Quick start

```bash
cargo install voicsh

# First run — detects your desktop and picks the best model:
voicsh init

# Mic mode:
voicsh                          # continuous mic → text into focused app
voicsh --once                   # single utterance → exit

# Pipe mode (no mic/runtime deps needed):
cat file.wav | voicsh

voicsh help                     # all commands and options
```

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

## Install

```bash
# Ubuntu/Debian:
sudo apt install build-essential cmake pkg-config libclang-dev libasound2-dev
cargo install voicsh
```

Other distros, GPU acceleration, and pipe-only builds: see [INSTALL.md](INSTALL.md).

## Text injection

voicsh injects transcribed text into your focused app. `voicsh init` auto-detects the best backend:

| Desktop | Backend | Notes |
|---------|---------|-------|
| GNOME 45+ | Portal (RemoteDesktop) | No extra tools needed |
| KDE 6.1+ | Portal or wtype | |
| Sway / Hyprland | wtype | `sudo apt install wtype` |
| Fallback | ydotool | Needs `ydotoold` daemon |

Override at runtime: `voicsh --injection-backend wtype`
Override via env: `VOICSH_BACKEND=portal voicsh`
Override in config: `[injection]` section — run `voicsh config dump` to see all options.

`wl-clipboard` (`wl-copy`) is required for clipboard-based injection.

> **Note:** wtype/ydotool inject via clipboard paste — this **overwrites your clipboard**. Portal types directly without touching the clipboard.

## Voice commands

Voice commands trigger only when spoken as **standalone utterances** — pause, say the command, pause. Text that merely contains a command word passes through unchanged:

```
[pause] "period" [pause]          → .
[pause] "new line" [pause]        → (line break)
"the period of history"           → "the period of history"
"press enter to continue"        → "press enter to continue"
[pause] "all caps" [pause] "wow" → "WOW"
```

Built-in commands are available for English, German, Spanish, French, Portuguese, Italian, Dutch, Polish, Russian, Japanese, Chinese, and Korean. Discover all commands for a language:

```bash
voicsh config list --language=en     # English voice commands
voicsh config list --language=ko     # Korean voice commands
voicsh config list --language=en,de  # multiple languages
```

Add custom commands in `[voice_commands.commands]` in config — they take precedence over built-ins. To disable voice commands entirely: `voice_commands.enabled = false`.

## Configuration

```bash
voicsh config dump              # commented template with all options and defaults
voicsh config list              # current active configuration
voicsh config list stt          # just the [stt] section
voicsh config get stt.model     # single value
voicsh config set stt.model small.en
```

Config file: `~/.config/voicsh/config.toml`. Environment overrides: `VOICSH_MODEL`, `VOICSH_LANGUAGE`, `VOICSH_BACKEND`.

## Shell integration

**GNOME extension** — panel indicator with recording state, model info, and Super+Alt+V toggle:

```bash
voicsh install-gnome-extension
```

**Shell completions:** `voicsh completions bash|zsh|fish` — run `voicsh completions --help` for install paths.

## License

MIT

## Acknowledgments

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Whisper inference engine
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- Inspired by [nerd-dictation](https://github.com/ideasman42/nerd-dictation), [voxd](https://github.com/voxd/voxd), and [BlahST](https://github.com/QuantiusBenignus/BlahST)
