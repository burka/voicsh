//! Streaming pipeline for continuous voice transcription.
//!
//! Implements a multi-station pipeline architecture:
//! ```text
//! ┌─────────────┐    ┌─────────────┐    ┌──────────┐    ┌───────────┐    ┌─────────┐
//! │  Continuous │───▶│  Silence    │───▶│ Chunker  │───▶│Transcriber│───▶│ Stitcher│───▶ Inject
//! │  Recording  │    │  Detector   │    │          │    │  (async)  │    │         │
//! └─────────────┘    └─────────────┘    └──────────┘    └───────────┘    └─────────┘
//!        │                  │                 ▲
//!        ▼                  │                 │
//!    Ring Buffer            └── control ──────┘
//!                              (flush chunk
//!                               on silence)
//! ```

pub mod chunker;
pub mod frame;
pub mod pipeline;
pub mod ring_buffer;
pub mod silence_detector;
pub mod stitcher;
pub mod transcriber;

pub use chunker::{ChunkerConfig, ChunkerStation};
pub use frame::{AudioFrame, ChunkData, ControlEvent, PipelineFrame};
pub use pipeline::{StreamingPipeline, StreamingPipelineConfig};
pub use ring_buffer::RingBuffer;
pub use silence_detector::SilenceDetectorStation;
pub use stitcher::StitcherStation;
pub use transcriber::TranscriberStation;
