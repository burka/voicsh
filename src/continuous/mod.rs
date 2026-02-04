//! Continuous audio pipeline for voice transcription.
//!
//! Implements a multi-station pipeline where each station runs in its own thread,
//! connected by bounded crossbeam channels for backpressure.

pub mod adaptive_chunker;
pub mod chunker_station;
pub mod error;
pub mod injector_station;
pub mod pipeline;
pub mod station;
pub mod transcriber_station;
pub mod types;
pub mod vad_station;

pub use chunker_station::ChunkerStation;
pub use error::{ErrorReporter, LogReporter, StationError};
pub use injector_station::InjectorStation;
pub use pipeline::{ContinuousPipeline, ContinuousPipelineConfig, ContinuousPipelineHandle};
pub use station::{Station, StationRunner};
pub use transcriber_station::TranscriberStation;
pub use types::{AudioChunk, AudioFrame, TranscribedText, VadFrame};
pub use vad_station::VadStation;
