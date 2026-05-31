# Integrating voicsh as a Transcription Sidecar

voicsh is a Rust binary and library crate, but the stable integration boundary
for non-Rust applications is the daemon IPC socket. That keeps Whisper model
loading, GPU/CPU backend selection, VAD, chunking, post-processing, and optional
error correction inside one long-running process while callers pass file paths
and receive plain JSON responses.

The source of truth is the implementation:

- IPC command and response JSON: `src/ipc/protocol.rs`
- Unix socket server framing and request handling: `src/ipc/server.rs`
- Daemon command dispatch and file transcription: `src/daemon/handler.rs`
- WAV parsing, mono conversion, and 16 kHz resampling: `src/audio/wav.rs`
- Pipeline wiring from audio source to VAD, chunker, transcriber, correction,
  post-processors, and sink: `src/pipeline/orchestrator.rs`
- CLI helper for manual socket calls: `src/cli.rs` and `src/main.rs`

## Request Flow

For a Go Telegram bot, keep media decoding in the Go service and use voicsh for
speech-to-text:

1. Download the Telegram voice file.
2. Convert Telegram OGG/Opus to WAV, mono, 16 kHz.
3. Send a `transcribe_file` IPC command containing the WAV path.
4. Read a single JSON response.
5. Delete the temporary media files from the Go process.

Telegram voice messages are normally OGG containers with Opus audio. voicsh's
file input currently accepts WAV. OGG support would be useful later, but WAV is
the intentionally narrow contract for this IPC command because `WavAudioSource`
already owns normalization into the PCM format Whisper expects.

Example command payload:

```json
{"type":"transcribe_file","path":"/tmp/telegram-voice.wav"}
```

Successful transcription response:

```json
{"type":"transcription","text":"hello from telegram"}
```

No detected speech response:

```json
{"type":"ok","message":"No speech detected"}
```

Failure response:

```json
{"type":"error","message":"Failed to open '/tmp/telegram-voice.wav': ..."}
```

The protocol is newline-delimited JSON. Send one JSON object followed by `\n`
and read until EOF. The socket path defaults to `$XDG_RUNTIME_DIR/voicsh.sock`,
falling back to `/tmp/voicsh-$uid.sock`; see `IpcServer::default_socket_path`
in `src/ipc/server.rs`.

## CLI Check

Start the daemon:

```bash
voicsh daemon
```

From another shell, transcribe a WAV file through the same socket API:

```bash
voicsh transcribe-file /tmp/telegram-voice.wav
```

This CLI command is only a convenience wrapper. A Go client should send the same
`Command::TranscribeFile` JSON directly to the Unix socket.

## OGG/Opus Handling

Keep OGG/Opus conversion outside voicsh for now. The practical Go-side choices
are:

- run `ffmpeg` as a subprocess,
- use a Go ffmpeg binding, or
- use an Opus decoder and WAV writer in Go.

The conversion target should be WAV, mono, 16 kHz. With ffmpeg:

```bash
ffmpeg -y -i voice.ogg -ac 1 -ar 16000 /tmp/telegram-voice.wav
```

Native Rust OGG/Opus support can be added later behind a separate audio source,
but it should not change the daemon IPC shape. The daemon should continue to
receive file paths, not large audio payloads, because the socket server has a
small command-size limit by design.

## Pipeline Contract

`transcribe_file` uses the same station pipeline as pipe mode:

```text
WAV file -> VAD -> adaptive chunker -> transcriber -> correction -> post-processors -> collector
```

For file input, microphone-specific auto-leveling and meter output are disabled.
The daemon reuses its loaded transcriber so repeated Telegram voice messages do
not pay model load time on every request.

The daemon command waits for the finite WAV source to finish naturally, then
cleans up the pipeline handle. That behavior lives in
`PipelineHandle::wait_for_result` and `DaemonCommandHandler::handle_transcribe_file`.

## Go Client Sketch

Pseudo-code:

```go
wavPath := convertTelegramOggToWav(oggPath)
conn, err := net.Dial("unix", socketPath)
if err != nil {
    return err
}
defer conn.Close()

fmt.Fprintf(conn, `{"type":"transcribe_file","path":%q}`+"\n", wavPath)

var resp struct {
    Type    string `json:"type"`
    Text    string `json:"text"`
    Message string `json:"message"`
}
if err := json.NewDecoder(conn).Decode(&resp); err != nil {
    return err
}

switch resp.Type {
case "transcription":
    return sendTelegramReply(resp.Text)
case "ok":
    return nil
case "error":
    return errors.New(resp.Message)
default:
    return fmt.Errorf("unexpected voicsh response type %q", resp.Type)
}
```

The Go service owns file lifecycle and permissions. Store temporary files in a
directory the daemon process can read, and remove them after the response.
