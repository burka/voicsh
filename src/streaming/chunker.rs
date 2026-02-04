//! Chunker station for the streaming pipeline.
//!
//! Accumulates audio frames and emits chunks when:
//! - Time threshold is reached (~3s default)
//! - FlushChunk control frame is received
//! - SpeechEnd control frame is received
//!
//! Maintains small overlap between chunks (~200ms) for word continuity.

use crate::defaults;
use crate::streaming::frame::{AudioFrame, ChunkData, ControlEvent, PipelineFrame};
use std::time::Instant;
use tokio::sync::mpsc;

/// Configuration for the chunker.
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Maximum chunk duration in milliseconds (default: 3000ms).
    pub chunk_duration_ms: u32,
    /// Overlap duration in milliseconds for word continuity (default: 200ms).
    pub overlap_ms: u32,
    /// Minimum chunk duration before emitting (default: 500ms).
    pub min_chunk_ms: u32,
    /// Sample rate for duration calculations.
    pub sample_rate: u32,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            chunk_duration_ms: 3000,
            overlap_ms: 200,
            min_chunk_ms: 500,
            sample_rate: defaults::SAMPLE_RATE,
        }
    }
}

/// Chunker that accumulates audio and emits chunks.
pub struct ChunkerStation {
    config: ChunkerConfig,
    /// Current chunk's audio samples.
    buffer: Vec<i16>,
    /// Samples to prepend to next chunk (overlap).
    overlap_buffer: Vec<i16>,
    /// Sequence of first frame in current chunk.
    start_sequence: Option<u64>,
    /// Sequence of last frame added.
    last_sequence: u64,
    /// Next chunk ID to emit.
    next_chunk_id: u64,
    /// Time when current chunk started.
    chunk_start_time: Option<Instant>,
    /// Whether speech has started (we only chunk during speech).
    speech_active: bool,
}

impl ChunkerStation {
    /// Creates a new chunker with default configuration.
    pub fn new() -> Self {
        Self::with_config(ChunkerConfig::default())
    }

    /// Creates a new chunker with custom configuration.
    pub fn with_config(config: ChunkerConfig) -> Self {
        Self {
            config,
            buffer: Vec::new(),
            overlap_buffer: Vec::new(),
            start_sequence: None,
            last_sequence: 0,
            next_chunk_id: 0,
            chunk_start_time: None,
            speech_active: false,
        }
    }

    /// Returns the current buffer duration in milliseconds.
    pub fn buffer_duration_ms(&self) -> u32 {
        (self.buffer.len() as u32 * 1000) / self.config.sample_rate
    }

    /// Calculates the number of samples for the overlap buffer.
    fn overlap_samples(&self) -> usize {
        (self.config.overlap_ms * self.config.sample_rate / 1000) as usize
    }

    /// Processes a pipeline frame and returns any chunks that should be emitted.
    pub fn process(&mut self, frame: PipelineFrame) -> Vec<ChunkData> {
        match frame {
            PipelineFrame::Audio(audio) => self.process_audio(audio),
            PipelineFrame::Control(control) => self.process_control(control),
            _ => Vec::new(),
        }
    }

    /// Processes an audio frame.
    fn process_audio(&mut self, frame: AudioFrame) -> Vec<ChunkData> {
        if !self.speech_active {
            return Vec::new();
        }

        // Track sequence numbers
        if self.start_sequence.is_none() {
            self.start_sequence = Some(frame.sequence);
            self.chunk_start_time = Some(Instant::now());

            // Prepend overlap from previous chunk
            if !self.overlap_buffer.is_empty() {
                self.buffer.extend_from_slice(&self.overlap_buffer);
                self.overlap_buffer.clear();
            }
        }
        self.last_sequence = frame.sequence;

        // Add samples to buffer
        self.buffer.extend_from_slice(&frame.samples);

        // Check if we should emit a chunk based on time
        let mut chunks = Vec::new();
        if self.buffer_duration_ms() >= self.config.chunk_duration_ms
            && let Some(chunk) = self.emit_chunk(false, false)
        {
            chunks.push(chunk);
        }

        chunks
    }

    /// Processes a control event.
    fn process_control(&mut self, control: ControlEvent) -> Vec<ChunkData> {
        match control {
            ControlEvent::SpeechStart => {
                self.speech_active = true;
                self.chunk_start_time = Some(Instant::now());
                Vec::new()
            }
            ControlEvent::FlushChunk => {
                // Emit current buffer if we have enough audio
                if self.speech_active
                    && self.buffer_duration_ms() >= self.config.min_chunk_ms
                    && let Some(chunk) = self.emit_chunk(true, false)
                {
                    return vec![chunk];
                }
                Vec::new()
            }
            ControlEvent::SpeechEnd => {
                self.speech_active = false;
                // Emit final chunk even if below minimum
                if !self.buffer.is_empty()
                    && let Some(chunk) = self.emit_chunk(true, true)
                {
                    return vec![chunk];
                }
                Vec::new()
            }
        }
    }

    /// Emits a chunk from the current buffer.
    fn emit_chunk(&mut self, flushed_early: bool, is_final: bool) -> Option<ChunkData> {
        if self.buffer.is_empty() {
            return None;
        }

        let chunk = ChunkData {
            chunk_id: self.next_chunk_id,
            start_sequence: self.start_sequence.unwrap_or(0),
            end_sequence: self.last_sequence,
            samples: std::mem::take(&mut self.buffer),
            flushed_early,
            is_final,
        };

        // Save overlap for next chunk (unless this is final)
        if !is_final {
            let overlap_samples = self.overlap_samples();
            if chunk.samples.len() > overlap_samples {
                self.overlap_buffer =
                    chunk.samples[chunk.samples.len() - overlap_samples..].to_vec();
            }
        }

        self.next_chunk_id += 1;
        self.start_sequence = None;
        self.chunk_start_time = None;

        Some(chunk)
    }

    /// Resets the chunker state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.overlap_buffer.clear();
        self.start_sequence = None;
        self.last_sequence = 0;
        self.next_chunk_id = 0;
        self.chunk_start_time = None;
        self.speech_active = false;
    }

    /// Runs the chunker as a station.
    ///
    /// # Arguments
    /// * `input` - Receiver for pipeline frames (audio + control)
    /// * `output` - Sender for chunk data
    pub async fn run(
        mut self,
        mut input: mpsc::Receiver<PipelineFrame>,
        output: mpsc::Sender<ChunkData>,
    ) {
        while let Some(frame) = input.recv().await {
            let chunks = self.process(frame);
            for chunk in chunks {
                let is_final = chunk.is_final;
                if output.send(chunk).await.is_err() {
                    return;
                }
                if is_final {
                    return;
                }
            }
        }
    }
}

impl Default for ChunkerStation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::frame::AudioFrame;

    fn make_audio_frame(seq: u64, samples: usize) -> PipelineFrame {
        PipelineFrame::Audio(AudioFrame::new(seq, vec![1000i16; samples]))
    }

    #[test]
    fn test_chunker_creation() {
        let chunker = ChunkerStation::new();
        assert_eq!(chunker.buffer_duration_ms(), 0);
        assert!(!chunker.speech_active);
    }

    #[test]
    fn test_chunker_config() {
        let config = ChunkerConfig {
            chunk_duration_ms: 2000,
            overlap_ms: 100,
            min_chunk_ms: 300,
            sample_rate: 16000,
        };
        let chunker = ChunkerStation::with_config(config);
        assert_eq!(chunker.config.chunk_duration_ms, 2000);
    }

    #[test]
    fn test_chunker_ignores_audio_before_speech() {
        let mut chunker = ChunkerStation::new();

        // Audio before SpeechStart should be ignored
        let chunks = chunker.process(make_audio_frame(0, 1600));
        assert!(chunks.is_empty());
        assert_eq!(chunker.buffer_duration_ms(), 0);
    }

    #[test]
    fn test_chunker_accumulates_after_speech_start() {
        let mut chunker = ChunkerStation::new();

        // Start speech
        let chunks = chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));
        assert!(chunks.is_empty());

        // Add audio (1600 samples at 16kHz = 100ms)
        let chunks = chunker.process(make_audio_frame(0, 1600));
        assert!(chunks.is_empty());
        assert_eq!(chunker.buffer_duration_ms(), 100);
    }

    #[test]
    fn test_chunker_emits_on_time_threshold() {
        let config = ChunkerConfig {
            chunk_duration_ms: 200, // Very short for testing
            overlap_ms: 20,
            min_chunk_ms: 50,
            sample_rate: 16000,
        };
        let mut chunker = ChunkerStation::with_config(config);

        // Start speech
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));

        // Add enough audio to exceed threshold (3200 samples = 200ms at 16kHz)
        let chunks = chunker.process(make_audio_frame(0, 3200));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_id, 0);
        assert!(!chunks[0].flushed_early);
        assert!(!chunks[0].is_final);
    }

    #[test]
    fn test_chunker_flush_on_control() {
        let config = ChunkerConfig {
            chunk_duration_ms: 3000,
            overlap_ms: 200,
            min_chunk_ms: 100,
            sample_rate: 16000,
        };
        let mut chunker = ChunkerStation::with_config(config);

        // Start speech
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));

        // Add some audio (1600 samples = 100ms, meets min_chunk_ms)
        chunker.process(make_audio_frame(0, 1600));
        assert_eq!(chunker.buffer_duration_ms(), 100);

        // Flush should emit chunk
        let chunks = chunker.process(PipelineFrame::Control(ControlEvent::FlushChunk));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].flushed_early);
        assert!(!chunks[0].is_final);
    }

    #[test]
    fn test_chunker_speech_end_emits_final() {
        let mut chunker = ChunkerStation::new();

        // Start speech and add audio
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));
        chunker.process(make_audio_frame(0, 1600));

        // Speech end should emit final chunk
        let chunks = chunker.process(PipelineFrame::Control(ControlEvent::SpeechEnd));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_final);
    }

    #[test]
    fn test_chunker_overlap_buffer() {
        let config = ChunkerConfig {
            chunk_duration_ms: 200,
            overlap_ms: 50, // 800 samples at 16kHz
            min_chunk_ms: 50,
            sample_rate: 16000,
        };
        let mut chunker = ChunkerStation::with_config(config);

        // Start speech
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));

        // Add audio and trigger chunk emission
        let chunks = chunker.process(make_audio_frame(0, 3200));
        assert_eq!(chunks.len(), 1);

        // Overlap buffer should be populated
        assert_eq!(chunker.overlap_buffer.len(), 800); // 50ms at 16kHz
    }

    #[test]
    fn test_chunker_reset() {
        let mut chunker = ChunkerStation::new();

        // Build up some state
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));
        chunker.process(make_audio_frame(0, 1600));

        // Reset
        chunker.reset();

        assert_eq!(chunker.buffer_duration_ms(), 0);
        assert!(!chunker.speech_active);
        assert_eq!(chunker.next_chunk_id, 0);
    }

    #[test]
    fn test_chunker_sequence_tracking() {
        let config = ChunkerConfig {
            chunk_duration_ms: 3000,
            overlap_ms: 200,
            min_chunk_ms: 100, // Low threshold for testing
            sample_rate: 16000,
        };
        let mut chunker = ChunkerStation::with_config(config);

        // Start speech
        chunker.process(PipelineFrame::Control(ControlEvent::SpeechStart));

        // Add frames with different sequences (1600 samples = 100ms each at 16kHz)
        chunker.process(make_audio_frame(10, 1600));
        chunker.process(make_audio_frame(11, 1600));
        chunker.process(make_audio_frame(12, 1600));

        // Flush - should emit since we have 300ms >= min_chunk_ms
        let chunks = chunker.process(PipelineFrame::Control(ControlEvent::FlushChunk));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_sequence, 10);
        assert_eq!(chunks[0].end_sequence, 12);
    }

    #[tokio::test]
    async fn test_chunker_run() {
        let config = ChunkerConfig {
            chunk_duration_ms: 100,
            overlap_ms: 20,
            min_chunk_ms: 50,
            sample_rate: 16000,
        };
        let chunker = ChunkerStation::with_config(config);

        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        // Run chunker in background
        tokio::spawn(async move {
            chunker.run(input_rx, output_tx).await;
        });

        // Send speech start
        input_tx
            .send(PipelineFrame::Control(ControlEvent::SpeechStart))
            .await
            .unwrap();

        // Send audio (1600 samples = 100ms = chunk threshold)
        input_tx.send(make_audio_frame(0, 1600)).await.unwrap();

        // Should receive a chunk
        let chunk = output_rx.recv().await.unwrap();
        assert_eq!(chunk.chunk_id, 0);

        // Send speech end
        input_tx
            .send(PipelineFrame::Control(ControlEvent::SpeechEnd))
            .await
            .unwrap();

        // Drop input to allow task to complete
        drop(input_tx);
    }
}
