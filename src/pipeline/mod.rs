//! Audio pipeline for voice transcription.
//!
//! Implements a multi-station pipeline where each station runs in its own thread,
//! connected by bounded crossbeam channels for backpressure.

pub mod adaptive_chunker;
pub mod chunker_station;
pub mod error;
pub mod latency;
pub mod orchestrator;
pub mod post_processor;
pub mod sink;
pub mod station;
pub mod transcriber_station;
pub mod types;
pub mod vad_station;

pub use chunker_station::ChunkerStation;
pub use error::{ErrorReporter, LogReporter, StationError};
pub use latency::{LatencyTracker, TranscriptionTiming};
pub use orchestrator::{Pipeline, PipelineConfig, PipelineHandle};
pub use post_processor::{
    PostProcessor, PostProcessorStation, VoiceCommandProcessor, build_post_processors,
};
pub use sink::{CollectorSink, InjectorSink, TextSink};
pub use station::{Station, StationRunner};
pub use transcriber_station::TranscriberStation;
pub use types::{AudioChunk, AudioFrame, SinkEvent, TranscribedText, VadFrame};
pub use vad_station::VadStation;
