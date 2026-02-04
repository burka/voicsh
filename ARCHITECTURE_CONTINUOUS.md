# Continuous Pipeline Architecture

## Overview

Replace the per-session streaming pipeline with a truly continuous audio pipeline that:
1. Starts audio capture ONCE at startup
2. Runs until shutdown
3. Emits transcriptions whenever speech ends

## Module Structure

```
src/continuous/
├── mod.rs                    # Module exports
├── station.rs                # Station trait + StationRunner
├── error.rs                  # StationError + ErrorReporter trait
├── types.rs                  # Shared types (AudioFrame, TranscribedText, etc.)
├── adaptive_chunker.rs       # Gap-shrinking chunker (SRP component)
├── vad_station.rs            # VAD as Station impl
├── chunker_station.rs        # Chunker as Station impl
├── transcriber_station.rs    # Whisper as Station impl
├── injector_station.rs       # Text injection as Station impl
└── pipeline.rs               # ContinuousPipeline orchestrator
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

## Pipeline Flow

```
┌──────────────┐     ┌─────────────┐     ┌─────────────┐     ┌────────────┐     ┌────────────┐
│ Audio Capture│────>│ VAD Station │────>│  Chunker    │────>│Transcriber │────>│  Injector  │
│  (CPAL cb)   │     │             │     │  Station    │     │  Station   │     │  Station   │
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
| Transcriber → Injector | 4 | Injection is fast |

## Shutdown Protocol

1. User sends Ctrl+C
2. Audio capture stops → input channel closes
3. Each station: sees closed input → finishes processing → closes output
4. Cascade propagates through pipeline
5. Main thread: join all with 1s timeout → force kill stragglers

## Dependencies to Add

```toml
crossbeam-channel = "0.5"
ringbuf = "0.4"  # Optional, for future zero-copy optimization
```

## Migration Plan

1. Create `src/continuous/` module
2. Implement and test each component
3. Add `run_record_continuous` to pipeline.rs
4. Update CLI to use new pipeline
5. Deprecate old `streaming/` module (remove later)
