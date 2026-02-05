//! Chunker station that segments speech into transcribable chunks.

use crate::audio::vad::{Clock, SystemClock};
use crate::pipeline::adaptive_chunker::{AdaptiveChunker, AdaptiveChunkerConfig};
use crate::pipeline::error::{StationError, eprintln_clear};
use crate::pipeline::station::Station;
use crate::pipeline::types::{AudioChunk, VadFrame};
use std::sync::Arc;
use std::time::Instant;

/// Station that segments VAD frames into speech chunks using adaptive gap detection.
///
/// This station:
/// - Tracks silence duration from VAD frames
/// - Feeds frames to AdaptiveChunker
/// - Emits AudioChunks when natural break points are found
/// - Assigns monotonically increasing sequence numbers
pub struct ChunkerStation {
    chunker: AdaptiveChunker,
    sequence: u64,
    sample_rate: u32,
    silence_start: Option<Instant>,
    verbose: bool,
    clock: Arc<dyn Clock>,
}

impl ChunkerStation {
    /// Creates a new chunker station with the given configuration.
    pub fn new(config: AdaptiveChunkerConfig) -> Self {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    /// Creates a new chunker station with an injectable clock.
    pub fn with_clock(config: AdaptiveChunkerConfig, clock: Arc<dyn Clock>) -> Self {
        let sample_rate = config.sample_rate;
        Self {
            chunker: AdaptiveChunker::with_clock(config, clock.clone()),
            sequence: 0,
            sample_rate,
            silence_start: None,
            verbose: false,
            clock,
        }
    }

    /// Sets a custom sample rate (overrides config value).
    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Enable diagnostic output to stderr.
    ///
    /// When verbose is true, diagnostic info is logged when chunks are emitted.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Flushes any remaining audio during shutdown.
    ///
    /// Call this to retrieve accumulated audio that hasn't been emitted yet.
    pub fn flush(&mut self) -> Option<AudioChunk> {
        self.chunker
            .flush()
            .map(|samples| self.create_chunk(samples))
    }

    /// Creates an AudioChunk from samples and increments sequence.
    fn create_chunk(&mut self, samples: Vec<i16>) -> AudioChunk {
        let duration_ms = self.calculate_duration_ms(samples.len());
        let seq = self.sequence;
        self.sequence += 1;
        let chunk = AudioChunk::new(samples, duration_ms, seq);

        // Log chunk emission if verbose
        if self.verbose {
            eprintln_clear(&format!(
                "  [chunk: {}ms, seq {}]",
                chunk.duration_ms, chunk.sequence
            ));
        }

        chunk
    }

    /// Calculates duration in milliseconds from sample count.
    fn calculate_duration_ms(&self, sample_count: usize) -> u32 {
        (sample_count as u32 * 1000) / self.sample_rate
    }

    /// Tracks silence duration based on VAD frame speech detection.
    fn update_silence_tracking(&mut self, is_speech: bool) {
        if is_speech {
            self.silence_start = None;
        } else if self.silence_start.is_none() {
            self.silence_start = Some(self.clock.now());
        }
    }

    /// Gets current silence duration in milliseconds.
    fn current_silence_ms(&self) -> u32 {
        match self.silence_start {
            Some(start) => self.clock.now().duration_since(start).as_millis() as u32,
            None => 0,
        }
    }
}

impl Station for ChunkerStation {
    type Input = VadFrame;
    type Output = AudioChunk;

    fn name(&self) -> &'static str {
        "chunker"
    }

    fn process(&mut self, frame: VadFrame) -> Result<Option<AudioChunk>, StationError> {
        // Update silence tracking
        self.update_silence_tracking(frame.is_speech);

        // Get current silence duration
        let silence_ms = self.current_silence_ms();

        // Feed frame to chunker
        let maybe_samples = self
            .chunker
            .feed(frame.is_speech, &frame.samples, silence_ms);

        // If chunker emitted samples, wrap in AudioChunk
        Ok(maybe_samples.map(|samples| self.create_chunk(samples)))
    }

    fn shutdown(&mut self) {
        // Flush is handled by the pipeline orchestrator through explicit flush() call
        // Note: Any remaining buffered audio can be retrieved via flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_config() -> AdaptiveChunkerConfig {
        AdaptiveChunkerConfig {
            target_chunk_ms: 2500,
            max_chunk_ms: 6000,
            initial_gap_ms: 400,
            min_gap_ms: 80,
            sample_rate: 16000,
        }
    }

    fn make_speech_frame(samples: Vec<i16>) -> VadFrame {
        VadFrame::new(samples, Instant::now(), true, 0.8)
    }

    fn make_silence_frame(samples: Vec<i16>) -> VadFrame {
        VadFrame::new(samples, Instant::now(), false, 0.1)
    }

    #[test]
    fn test_chunker_station_name() {
        let config = make_test_config();
        let station = ChunkerStation::new(config);
        assert_eq!(station.name(), "chunker");
    }

    #[test]
    fn test_accumulates_speech_frames() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        let samples = vec![1; 16000]; // 1 second of audio
        let frame = make_speech_frame(samples.clone());

        // First frame should not emit (below target duration)
        let result = station.process(frame).unwrap();
        assert!(result.is_none());

        // Verify accumulation (should have ~1000ms)
        let accumulated = station.chunker.accumulated_duration_ms();
        assert!((900..=1100).contains(&accumulated));
    }

    #[test]
    fn test_emits_chunk_on_gap_threshold() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        // Generate enough samples to exceed target duration (3 seconds)
        let samples_per_second = 16000;
        let samples: Vec<i16> = (0..samples_per_second).map(|i| i as i16).collect();

        // Feed 3 seconds of speech with delays to let time pass
        for _ in 0..3 {
            let frame = make_speech_frame(samples.clone());
            let result = station.process(frame).unwrap();
            assert!(result.is_none());
            std::thread::sleep(std::time::Duration::from_millis(1001));
        }

        // Feed a silence frame to start silence tracking
        let silence_frame = make_silence_frame(samples.clone());
        let result = station.process(silence_frame).unwrap();
        assert!(result.is_none()); // First silence frame won't emit yet

        // Wait for 250ms+ (required gap at 3000ms is 250ms)
        std::thread::sleep(std::time::Duration::from_millis(260));

        // Feed another silence frame - now we should emit
        let silence_frame = make_silence_frame(samples.clone());
        let result = station.process(silence_frame).unwrap();

        // Should emit a chunk because silence threshold met
        assert!(result.is_some());
        let chunk = result.unwrap();
        assert!(!chunk.samples.is_empty());
        assert_eq!(chunk.sequence, 0);
    }

    #[test]
    fn test_sequence_numbers_increment() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        let samples_per_second = 16000;
        let samples: Vec<i16> = (0..samples_per_second).map(|i| i as i16).collect();

        // Emit first chunk by accumulating speech then silence
        for _ in 0..3 {
            let frame = make_speech_frame(samples.clone());
            station.process(frame).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1001));
        }

        // Start silence tracking
        let silence_frame = make_silence_frame(samples.clone());
        station.process(silence_frame).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(260));

        let silence_frame = make_silence_frame(samples.clone());
        let chunk1 = station.process(silence_frame).unwrap();
        assert!(chunk1.is_some(), "First chunk should emit");
        assert_eq!(chunk1.unwrap().sequence, 0);

        // Emit second chunk
        for _ in 0..3 {
            let frame = make_speech_frame(samples.clone());
            station.process(frame).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1001));
        }

        // Start silence tracking for second chunk
        let silence_frame = make_silence_frame(samples.clone());
        station.process(silence_frame).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(260));

        let silence_frame = make_silence_frame(samples.clone());
        let chunk2 = station.process(silence_frame).unwrap();
        assert!(chunk2.is_some(), "Second chunk should emit");
        assert_eq!(chunk2.unwrap().sequence, 1);
    }

    #[test]
    fn test_flush_returns_remaining_audio() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        let samples = vec![1, 2, 3, 4, 5];
        let frame = make_speech_frame(samples.clone());

        // Accumulate some audio
        station.process(frame.clone()).unwrap();
        station.process(frame.clone()).unwrap();

        // Flush should return accumulated audio
        let chunk = station.flush();
        assert!(chunk.is_some());

        let chunk = chunk.unwrap();
        assert_eq!(chunk.samples.len(), 10); // 2 frames of 5 samples each
        assert_eq!(chunk.sequence, 0);
    }

    #[test]
    fn test_flush_returns_none_when_empty() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        // Flush without accumulating should return None
        let chunk = station.flush();
        assert!(chunk.is_none());
    }

    #[test]
    fn test_silence_tracking() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        let samples = vec![1, 2, 3, 4, 5];

        // Speech frame - silence tracking should be None
        let speech_frame = make_speech_frame(samples.clone());
        station.process(speech_frame).unwrap();
        assert!(station.silence_start.is_none());

        // Silence frame - silence tracking should start
        let silence_frame = make_silence_frame(samples.clone());
        station.process(silence_frame).unwrap();
        assert!(station.silence_start.is_some());

        // Another speech frame - silence tracking should reset
        let speech_frame = make_speech_frame(samples.clone());
        station.process(speech_frame).unwrap();
        assert!(station.silence_start.is_none());
    }

    #[test]
    fn test_ignores_silence_before_speech() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        let samples = vec![0, 0, 0, 0, 0];
        let silence_frame = make_silence_frame(samples);

        // Silence before speech should not emit or accumulate
        let result = station.process(silence_frame).unwrap();
        assert!(result.is_none());
        assert_eq!(station.chunker.accumulated_duration_ms(), 0);
    }

    #[test]
    fn test_duration_calculation() {
        let config = make_test_config();
        let station = ChunkerStation::new(config);

        // 16000 samples at 16kHz = 1000ms
        assert_eq!(station.calculate_duration_ms(16000), 1000);

        // 8000 samples at 16kHz = 500ms
        assert_eq!(station.calculate_duration_ms(8000), 500);

        // 32000 samples at 16kHz = 2000ms
        assert_eq!(station.calculate_duration_ms(32000), 2000);
    }

    #[test]
    fn test_with_sample_rate() {
        let config = make_test_config();
        let station = ChunkerStation::new(config).with_sample_rate(48000);

        assert_eq!(station.sample_rate, 48000);

        // 48000 samples at 48kHz = 1000ms
        assert_eq!(station.calculate_duration_ms(48000), 1000);
    }

    #[test]
    fn test_chunk_duration_matches_samples() {
        let config = make_test_config();
        let mut station = ChunkerStation::new(config);

        // Create a chunk with known sample count
        let samples = vec![1; 16000]; // 1 second at 16kHz
        let chunk = station.create_chunk(samples);

        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.samples.len(), 16000);
        assert_eq!(chunk.sequence, 0);
    }
}
