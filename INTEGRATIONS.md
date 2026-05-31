# Integrations

Use the daemon socket when another program, such as a Go Telegram bot, needs
voicsh transcription without reloading the model for every message.

## Contract

Send one newline-delimited JSON command to the Unix socket. For the simplest
blocking call, read one JSON response:

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

For multiple clients, use daemon jobs. `submit_transcribe_file` subscribes by
default, so the same socket streams status updates until the job finishes:

```json
{"type":"submit_transcribe_file","path":"/tmp/voice.wav"}
{"type":"job_update","job":{"job_id":"job-1","state":"queued", "...":"..."}}
{"type":"job_update","job":{"job_id":"job-1","state":"done","text":"hello", "...":"..."}}
```

Use `{"type":"submit_transcribe_file","path":"/tmp/voice.wav","subscribe":false}`
for submit-and-close. Reconnect with `job_status`, `job_result`, or
`job_subscribe`; `job_subscribe` replays the current state by default. Jobs are
in-memory, so resubmit after a daemon restart.

## Telegram Voice Messages

Telegram voice messages are usually OGG/Opus. Convert them outside voicsh to
mono 16 kHz WAV, then submit the WAV path:

```bash
ffmpeg -y -i voice.ogg -ac 1 -ar 16000 /tmp/voice.wav
```

The Go service should own download, conversion, temporary-file permissions, and
cleanup. Keep payloads out of IPC; pass file paths.

## Source Of Truth

- JSON protocol: `src/ipc/protocol.rs`
- Socket framing, limits, and default socket path: `src/ipc/server.rs`
- Job queue and file transcription behavior: `src/daemon/jobs.rs`, `src/daemon/handler.rs`
- WAV parsing and resampling: `src/audio/wav.rs`
- Pipeline lifecycle: `src/pipeline/orchestrator.rs`
- CLI wrapper: `src/cli.rs`, `src/main.rs`
