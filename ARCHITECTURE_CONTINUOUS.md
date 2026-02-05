# Pipeline Architecture

## Overview

A unified, continuous audio pipeline that:
1. Starts audio capture once at initialization
2. Runs until shutdown
3. Emits transcriptions whenever speech ends
4. Pluggable output sinks (voice typing, text accumulation, custom handlers)

## Module Structure

```
src/pipeline/
├── mod.rs                    # Module exports
├── orchestrator.rs           # Pipeline, PipelineConfig, PipelineHandle
├── station.rs                # Station trait + StationRunner
├── sink.rs                   # TextSink trait, SinkStation, InjectorSink, CollectorSink
├── error.rs                  # StationError + ErrorReporter trait
├── types.rs                  # Shared types (AudioFrame, TranscribedText, etc.)
├── adaptive_chunker.rs       # Gap-shrinking chunker (SRP component)
├── vad_station.rs            # VAD as Station impl
├── chunker_station.rs        # Chunker as Station impl
└── transcriber_station.rs    # Whisper as Station impl
```

## Core Abstractions

### Station Trait (station.rs)

```rust
pub trait Station: Send + 'static {
    type Input: Send + 'static;
    type Output: Send + 'static;

    /// Process one input, optionally producing output
    fn process(&mut self, input: Self::Input) -> Result<Option<Self::Output>, StationError>;

    /// Station name for error reporting
    fn name(&self) -> &'static str;

    /// Called on graceful shutdown
    fn shutdown(&mut self) {}
}
```

### StationRunner (station.rs)

Spawns a station in its own thread, connecting input/output channels:

```rust
pub struct StationRunner {
    handle: JoinHandle<()>,
    name: &'static str,
}

impl StationRunner {
    pub fn spawn<S: Station>(
        station: S,
        input: Receiver<S::Input>,
        output: Sender<S::Output>,
        error_reporter: Arc<dyn ErrorReporter>,
    ) -> Self;

    pub fn join(self) -> thread::Result<()>;
}
```

### ErrorReporter Trait (error.rs)

```rust
pub trait ErrorReporter: Send + Sync + 'static {
    fn report(&self, station: &str, error: &StationError);
}

pub struct LogReporter;  // Default: logs to stderr
```

### StationError (error.rs)

```rust
pub enum StationError {
    /// Recoverable error - log and continue
    Recoverable(String),
    /// Fatal error - station should stop
    Fatal(String),
}
```

### TextSink Trait (sink.rs)

Pluggable output for transcribed text:

```rust
pub trait TextSink: Send + 'static {
    /// Process transcribed text
    fn handle(&mut self, text: &str);

    /// Called on pipeline shutdown
    /// Returns optional string (e.g., accumulated text for --once mode)
    fn finish(&mut self) -> Option<String>;

    /// Sink name for logging
    fn name(&self) -> &'static str;
}
```

Implementations:
- `InjectorSink<E>` - Voice typing via clipboard/direct injection
- `CollectorSink` - Accumulates text, returns on finish() (for --once mode)
- Custom sinks via trait objects

## Data Types (types.rs)

```rust
/// Raw audio frame from capture
pub struct AudioFrame {
    pub samples: Vec<i16>,
    pub timestamp: Instant,
    pub sequence: u64,
}

/// Audio with VAD annotation
pub struct VadFrame {
    pub samples: Vec<i16>,
    pub timestamp: Instant,
    pub is_speech: bool,
    pub level: f32,
}

/// Accumulated chunk ready for transcription
pub struct AudioChunk {
    pub samples: Vec<i16>,
    pub duration_ms: u32,
    pub sequence: u64,
}

/// Transcription result
pub struct TranscribedText {
    pub text: String,
    pub timestamp: Instant,
}
```

## Pipeline Orchestration (orchestrator.rs)

### PipelineConfig

```rust
pub struct PipelineConfig {
    pub sample_rate: u32,
    pub frame_duration_ms: u32,
    pub vad_silence_threshold_db: f32,
    pub audio_vad_channel_size: usize,
    pub vad_chunker_channel_size: usize,
    pub chunker_transcriber_channel_size: usize,
    pub transcriber_sink_channel_size: usize,
}
```

### Pipeline

```rust
pub struct Pipeline {
    config: PipelineConfig,
}

impl Pipeline {
    pub fn new(config: PipelineConfig) -> Self;

    /// Start the pipeline with pluggable sources and sink
    pub fn start(
        self,
        audio_source: Box<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        sink: Box<dyn TextSink>,
    ) -> Result<PipelineHandle>;
}
```

### PipelineHandle

```rust
pub struct PipelineHandle {
    // ... internal runners and channels ...
}

impl PipelineHandle {
    /// Stop the pipeline and return result from sink.finish()
    pub fn stop(self) -> Option<String>;
}
```

## Pipeline Flow

```
┌──────────────┐     ┌─────────────┐     ┌─────────────┐     ┌────────────┐     ┌────────────┐
│ Audio Capture│────>│ VAD Station │────>│  Chunker    │────>│Transcriber │────>│SinkStation │
│ (AudioSource)│     │             │     │  Station    │     │  Station   │     │ (TextSink) │
└──────────────┘     └─────────────┘     └─────────────┘     └────────────┘     └────────────┘
       │                   │                   │                   │                  │
       └───────────────────┴───────────────────┴───────────────────┴──────────────────┘
                                    All channels: crossbeam::channel::bounded
                                    All errors: Arc<dyn ErrorReporter>
```

## Channel Sizes (Backpressure)

| Channel | Buffer Size | Rationale |
|---------|-------------|-----------|
| Audio → VAD | 32 | ~1.3s at 40ms frames |
| VAD → Chunker | 16 | VAD doesn't filter much |
| Chunker → Transcriber | 4 | Transcription is slow |
| Transcriber → Sink | 4 | Sink processing is fast |

## Shutdown Protocol

1. User sends Ctrl+C
2. Audio capture stops → input channel closes
3. Each station: sees closed input → finishes processing → closes output
4. Cascade propagates through pipeline
5. SinkStation calls TextSink::finish(), stores result
6. Main thread: join all with 1s timeout → force kill stragglers
7. PipelineHandle::stop() returns result from sink.finish()

## Library API

```rust
use voicsh::{Pipeline, PipelineConfig, AudioSource, Transcriber, TextSink};

// With default config
let pipeline = Pipeline::new(PipelineConfig::default());

// Start the pipeline
let handle = pipeline.start(
    Box::new(my_audio_source),
    Arc::new(my_transcriber),
    Box::new(my_sink),
)?;

// Stop and get result from sink
let result: Option<String> = handle.stop();
```

## Adaptive Chunker Algorithm (adaptive_chunker.rs)

Gap-shrinking algorithm for natural speech chunking:

```
Required silence gap = f(speech_duration)

At 2.5s of speech: require 400ms silence (sentence boundaries)
At 3.0s: require 250ms (clause boundaries)
At 3.5s: require 150ms (inter-word gaps)
At 4.0s: require 100ms (safe minimum)
At 4.5s+: require 80ms (floor - never lower)
```

This is a **pure function** component - no I/O, fully testable.

## Feature Gates

```toml
[features]
default = ["full"]
full = ["cpal-audio", "whisper", "model-download", "cli"]
cpal-audio = []
whisper = []
model-download = []
cli = []
```

Feature combinations allow library consumers to use the core pipeline without CLI dependencies, and enable optional audio/transcription backends.
