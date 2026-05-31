# Integrations

Use the daemon socket when another program, such as a Go Telegram bot, needs
voicsh transcription without reloading the model for every message.

## Contract

Send one newline-delimited JSON command to the Unix socket and read one JSON
response:

```json
{"type":"transcribe_file","path":"/tmp/voice.wav"}
```

Responses are the normal IPC responses:

```json
{"type":"transcription","text":"hello"}
{"type":"ok","message":"No speech detected"}
{"type":"error","message":"..."}
```

The file must be a regular WAV file readable by the daemon. File input is capped
before parsing because WAV data is read into memory.

## Telegram Voice Messages

Telegram voice messages are usually OGG/Opus. Convert them outside voicsh to
mono 16 kHz WAV, then call `transcribe_file`:

```bash
ffmpeg -y -i voice.ogg -ac 1 -ar 16000 /tmp/voice.wav
voicsh transcribe-file /tmp/voice.wav
```

The Go service should own download, conversion, temporary-file permissions, and
cleanup. Keep payloads out of IPC; pass file paths.

## Source Of Truth

- JSON protocol: `src/ipc/protocol.rs`
- Socket framing, limits, and default socket path: `src/ipc/server.rs`
- File transcription behavior: `src/daemon/handler.rs`
- WAV parsing and resampling: `src/audio/wav.rs`
- Pipeline lifecycle: `src/pipeline/orchestrator.rs`
- CLI wrapper: `src/cli.rs`, `src/main.rs`
