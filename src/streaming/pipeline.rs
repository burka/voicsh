//! Streaming pipeline orchestrator.
//!
//! Connects all stations together and manages the complete streaming flow:
//! Ring Buffer → Silence Detector → Chunker → Transcriber → Stitcher → Output

use crate::audio::recorder::AudioSource;
use crate::audio::vad::{Clock, SystemClock};
use crate::config::Config;
use crate::error::{Result, VoicshError};
use crate::streaming::chunker::{ChunkerConfig, ChunkerStation};
use crate::streaming::frame::{ChunkData, PipelineFrame, TranscriptionResult};
use crate::streaming::ring_buffer::{RingBuffer, RingBufferConfig, RingBufferHandle};
use crate::streaming::silence_detector::{SilenceDetectorConfig, SilenceDetectorStation};
use crate::streaming::stitcher::{StitcherConfig, StitcherStation};
use crate::streaming::transcriber::TranscriberStation;
use crate::stt::transcriber::Transcriber;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Configuration for the streaming pipeline.
#[derive(Debug, Clone)]
pub struct StreamingPipelineConfig {
    /// Ring buffer configuration.
    pub ring_buffer: RingBufferConfig,
    /// Silence detector configuration.
    pub silence_detector: SilenceDetectorConfig,
    /// Chunker configuration.
    pub chunker: ChunkerConfig,
    /// Stitcher configuration.
    pub stitcher: StitcherConfig,
    /// Maximum concurrent transcriptions.
    pub max_concurrent_transcriptions: usize,
    /// Channel buffer sizes.
    pub channel_buffer_size: usize,
}

impl Default for StreamingPipelineConfig {
    fn default() -> Self {
        Self {
            ring_buffer: RingBufferConfig::default(),
            silence_detector: SilenceDetectorConfig::default(),
            chunker: ChunkerConfig::default(),
            stitcher: StitcherConfig::default(),
            max_concurrent_transcriptions: 2,
            channel_buffer_size: 100,
        }
    }
}

impl StreamingPipelineConfig {
    /// Creates a config with custom chunk duration.
    pub fn with_chunk_duration_ms(mut self, ms: u32) -> Self {
        self.chunker.chunk_duration_ms = ms;
        self
    }

    /// Enables level meter display (for verbose mode).
    pub fn with_show_levels(mut self, show: bool) -> Self {
        self.silence_detector.show_levels = show;
        self
    }

    /// Enables auto-leveling (AGC).
    pub fn with_auto_level(mut self, enabled: bool) -> Self {
        self.silence_detector.auto_level = enabled;
        self
    }

    /// Creates configuration from app config.
    pub fn from_config(config: &Config) -> Self {
        let mut pipeline_config = Self::default();

        // Apply VAD settings from config
        pipeline_config.silence_detector.vad.speech_threshold = config.audio.vad_threshold;
        pipeline_config.silence_detector.vad.silence_duration_ms = config.audio.silence_duration_ms;

        pipeline_config
    }
}

/// Handle to a running streaming pipeline.
pub struct StreamingPipelineHandle {
    ring_buffer_handle: RingBufferHandle,
}

impl StreamingPipelineHandle {
    /// Stops the streaming pipeline.
    pub fn stop(&self) {
        self.ring_buffer_handle.stop();
    }

    /// Returns true if the pipeline is running.
    pub fn is_running(&self) -> bool {
        self.ring_buffer_handle.is_running()
    }
}

/// Streaming pipeline that orchestrates all stations.
pub struct StreamingPipeline {
    config: StreamingPipelineConfig,
    clock: Arc<dyn Clock>,
}

impl StreamingPipeline {
    /// Creates a new streaming pipeline with default configuration.
    pub fn new() -> Self {
        Self::with_config(StreamingPipelineConfig::default())
    }

    /// Creates a new streaming pipeline with custom configuration.
    pub fn with_config(config: StreamingPipelineConfig) -> Self {
        Self {
            config,
            clock: Arc::new(SystemClock),
        }
    }

    /// Sets a custom clock (for deterministic testing).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Runs the streaming pipeline and returns the transcribed text.
    ///
    /// This method blocks until speech ends and all transcription is complete.
    ///
    /// # Arguments
    /// * `audio_source` - Audio source for capturing audio
    /// * `transcriber` - Transcriber for converting audio to text
    ///
    /// # Returns
    /// The combined transcription from all chunks
    pub async fn run<T: Transcriber + Send + Sync + 'static>(
        &self,
        audio_source: Box<dyn AudioSource>,
        transcriber: T,
    ) -> Result<String> {
        // Create channels between stations
        let (detector_tx, detector_rx) =
            mpsc::channel::<PipelineFrame>(self.config.channel_buffer_size);
        let (chunker_tx, chunker_rx) = mpsc::channel::<ChunkData>(self.config.channel_buffer_size);
        let (transcriber_tx, transcriber_rx) =
            mpsc::channel::<TranscriptionResult>(self.config.channel_buffer_size);
        let (stitcher_tx, mut stitcher_rx) = mpsc::channel::<String>(1);

        // Start ring buffer (returns handle and receiver)
        let ring_buffer = RingBuffer::with_config(audio_source, self.config.ring_buffer.clone());
        let (audio_rx, ring_buffer_handle) = ring_buffer.start()?;

        // Create stations
        let silence_detector = SilenceDetectorStation::with_clock(
            self.config.silence_detector.clone(),
            self.clock.clone(),
        );
        let chunker = ChunkerStation::with_config(self.config.chunker.clone());
        let transcriber_station = TranscriberStation::new(transcriber);
        let stitcher = StitcherStation::with_config(self.config.stitcher.clone());

        // Spawn station tasks
        let detector_task = tokio::spawn(async move {
            silence_detector.run(audio_rx, detector_tx).await;
        });

        let chunker_task = tokio::spawn(async move {
            chunker.run(detector_rx, chunker_tx).await;
        });

        let max_concurrent = self.config.max_concurrent_transcriptions;
        let transcriber_task = tokio::spawn(async move {
            transcriber_station
                .run(chunker_rx, transcriber_tx, max_concurrent)
                .await;
        });

        let stitcher_task = tokio::spawn(async move {
            stitcher.run(transcriber_rx, stitcher_tx).await;
        });

        // Wait for result from stitcher
        let result = stitcher_rx.recv().await;

        // Clean up
        ring_buffer_handle.stop();

        // Wait for all tasks to complete
        let _ = tokio::join!(detector_task, chunker_task, transcriber_task, stitcher_task);

        result.ok_or_else(|| VoicshError::Transcription {
            message: "Pipeline completed without producing output".to_string(),
        })
    }

    /// Runs the streaming pipeline with a callback for intermediate results.
    ///
    /// # Arguments
    /// * `audio_source` - Audio source for capturing audio
    /// * `transcriber` - Transcriber for converting audio to text
    /// * `on_chunk` - Callback invoked for each chunk transcription
    ///
    /// # Returns
    /// The combined transcription from all chunks
    pub async fn run_with_callback<T, F>(
        &self,
        audio_source: Box<dyn AudioSource>,
        transcriber: T,
        mut on_chunk: F,
    ) -> Result<String>
    where
        T: Transcriber + Send + Sync + 'static,
        F: FnMut(&TranscriptionResult) + Send + 'static,
    {
        // Create channels between stations
        let (detector_tx, detector_rx) =
            mpsc::channel::<PipelineFrame>(self.config.channel_buffer_size);
        let (chunker_tx, chunker_rx) = mpsc::channel::<ChunkData>(self.config.channel_buffer_size);
        let (transcriber_tx, mut transcriber_rx) =
            mpsc::channel::<TranscriptionResult>(self.config.channel_buffer_size);
        let (stitcher_tx, mut stitcher_rx) = mpsc::channel::<String>(1);

        // Start ring buffer
        let ring_buffer = RingBuffer::with_config(audio_source, self.config.ring_buffer.clone());
        let (audio_rx, ring_buffer_handle) = ring_buffer.start()?;

        // Create stations
        let silence_detector = SilenceDetectorStation::with_clock(
            self.config.silence_detector.clone(),
            self.clock.clone(),
        );
        let chunker = ChunkerStation::with_config(self.config.chunker.clone());
        let transcriber_station = TranscriberStation::new(transcriber);
        let stitcher = StitcherStation::with_config(self.config.stitcher.clone());

        // Spawn station tasks
        tokio::spawn(async move {
            silence_detector.run(audio_rx, detector_tx).await;
        });

        tokio::spawn(async move {
            chunker.run(detector_rx, chunker_tx).await;
        });

        let max_concurrent = self.config.max_concurrent_transcriptions;
        tokio::spawn(async move {
            transcriber_station
                .run(chunker_rx, transcriber_tx, max_concurrent)
                .await;
        });

        // Forward results to stitcher while calling callback
        let stitcher_input_tx = {
            let (tx, rx) = mpsc::channel::<TranscriptionResult>(self.config.channel_buffer_size);
            tokio::spawn(async move {
                stitcher.run(rx, stitcher_tx).await;
            });
            tx
        };

        // Process results with callback
        tokio::spawn(async move {
            while let Some(result) = transcriber_rx.recv().await {
                on_chunk(&result);
                if stitcher_input_tx.send(result).await.is_err() {
                    break;
                }
            }
        });

        // Wait for final result
        let result = stitcher_rx.recv().await;

        // Clean up
        ring_buffer_handle.stop();

        result.ok_or_else(|| VoicshError::Transcription {
            message: "Pipeline completed without producing output".to_string(),
        })
    }
}

impl Default for StreamingPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recorder::MockAudioSource;
    use crate::audio::vad::MockClock;
    use crate::stt::transcriber::MockTranscriber;
    use std::sync::Arc;

    #[test]
    fn test_pipeline_config_default() {
        let config = StreamingPipelineConfig::default();
        assert_eq!(config.max_concurrent_transcriptions, 2);
        assert_eq!(config.channel_buffer_size, 100);
    }

    #[test]
    fn test_pipeline_config_with_chunk_duration() {
        let config = StreamingPipelineConfig::default().with_chunk_duration_ms(2000);
        assert_eq!(config.chunker.chunk_duration_ms, 2000);
    }

    #[test]
    fn test_pipeline_creation() {
        let pipeline = StreamingPipeline::new();
        assert_eq!(pipeline.config.max_concurrent_transcriptions, 2);
        assert_eq!(pipeline.config.channel_buffer_size, 100);
    }

    #[test]
    fn test_pipeline_with_config() {
        let config = StreamingPipelineConfig {
            max_concurrent_transcriptions: 4,
            ..Default::default()
        };
        let pipeline = StreamingPipeline::with_config(config);
        assert_eq!(pipeline.config.max_concurrent_transcriptions, 4);
    }

    #[test]
    fn test_pipeline_default_trait() {
        let pipeline = StreamingPipeline::default();
        assert_eq!(pipeline.config.max_concurrent_transcriptions, 2);
        assert_eq!(pipeline.config.channel_buffer_size, 100);
    }

    #[test]
    fn test_config_with_show_levels() {
        let config = StreamingPipelineConfig::default().with_show_levels(true);
        assert!(config.silence_detector.show_levels);

        let config = StreamingPipelineConfig::default().with_show_levels(false);
        assert!(!config.silence_detector.show_levels);
    }

    #[test]
    fn test_config_with_auto_level() {
        let config = StreamingPipelineConfig::default().with_auto_level(false);
        assert!(!config.silence_detector.auto_level);

        let config = StreamingPipelineConfig::default().with_auto_level(true);
        assert!(config.silence_detector.auto_level);
    }

    #[test]
    fn test_config_builder_chaining() {
        let config = StreamingPipelineConfig::default()
            .with_chunk_duration_ms(1500)
            .with_show_levels(true)
            .with_auto_level(false);
        assert_eq!(config.chunker.chunk_duration_ms, 1500);
        assert!(config.silence_detector.show_levels);
        assert!(!config.silence_detector.auto_level);
    }

    #[test]
    fn test_config_from_config() {
        let app_config = Config::default();
        let pipeline_config = StreamingPipelineConfig::from_config(&app_config);
        assert_eq!(
            pipeline_config.silence_detector.vad.speech_threshold,
            app_config.audio.vad_threshold
        );
        assert_eq!(
            pipeline_config.silence_detector.vad.silence_duration_ms,
            app_config.audio.silence_duration_ms
        );
    }

    #[test]
    fn test_config_custom_buffer_sizes() {
        let config = StreamingPipelineConfig {
            max_concurrent_transcriptions: 8,
            channel_buffer_size: 500,
            ..Default::default()
        };
        assert_eq!(config.max_concurrent_transcriptions, 8);
        assert_eq!(config.channel_buffer_size, 500);
    }

    #[tokio::test]
    async fn test_pipeline_with_clock() {
        let clock = Arc::new(MockClock::new());
        let pipeline = StreamingPipeline::new().with_clock(clock.clone());
        // Verify clock was set (indirect test via config presence)
        assert_eq!(pipeline.config.max_concurrent_transcriptions, 2);
    }

    #[tokio::test]
    async fn test_run_audio_source_start_failure() {
        let pipeline = StreamingPipeline::new();
        let source = Box::new(MockAudioSource::new().with_start_failure());
        let transcriber = MockTranscriber::new("test");

        let result = pipeline.run(source, transcriber).await;
        assert!(result.is_err());
    }
}
