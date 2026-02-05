//! Adaptive chunker with gap-shrinking algorithm for natural speech segmentation.
//!
//! Uses research-based thresholds to find natural break points in speech,
//! becoming more aggressive about finding gaps as speech duration increases.

use std::time::Instant;

/// Configuration for the adaptive chunker.
#[derive(Debug, Clone)]
pub struct AdaptiveChunkerConfig {
    /// Target duration before actively looking for gaps (ms)
    pub target_chunk_ms: u32,
    /// Maximum duration before forcing a chunk emit (ms)
    pub max_chunk_ms: u32,
    /// Gap required at target_chunk_ms (ms)
    pub initial_gap_ms: u32,
    /// Minimum gap threshold floor (ms) - never go below this
    pub min_gap_ms: u32,
    /// Audio sample rate for duration calculations
    pub sample_rate: u32,
}

impl Default for AdaptiveChunkerConfig {
    fn default() -> Self {
        Self {
            target_chunk_ms: 2500,
            max_chunk_ms: 6000,
            initial_gap_ms: 400,
            min_gap_ms: 80,
            sample_rate: 16000,
        }
    }
}

/// State of the chunker
enum ChunkerState {
    /// Waiting for speech to start
    Idle,
    /// Accumulating speech audio
    Accumulating {
        samples: Vec<i16>,
        speech_start: Instant,
        silence_start: Option<Instant>,
    },
}

/// Adaptive chunker that emits audio chunks at natural break points.
pub struct AdaptiveChunker {
    config: AdaptiveChunkerConfig,
    state: ChunkerState,
}

impl AdaptiveChunker {
    pub fn new(config: AdaptiveChunkerConfig) -> Self {
        Self {
            config,
            state: ChunkerState::Idle,
        }
    }

    /// Feed a frame with VAD annotation. Returns Some(chunk) when ready to emit.
    ///
    /// # Arguments
    /// * `is_speech` - VAD says this frame contains speech
    /// * `samples` - Audio samples
    /// * `current_silence_ms` - How long silence has been detected (from VAD)
    pub fn feed(
        &mut self,
        is_speech: bool,
        samples: &[i16],
        current_silence_ms: u32,
    ) -> Option<Vec<i16>> {
        match &mut self.state {
            ChunkerState::Idle => {
                if is_speech {
                    // Start accumulating
                    self.state = ChunkerState::Accumulating {
                        samples: samples.to_vec(),
                        speech_start: Instant::now(),
                        silence_start: None,
                    };
                }
                None
            }
            ChunkerState::Accumulating {
                samples: buffer,
                speech_start,
                silence_start,
            } => {
                // Always add samples to buffer
                buffer.extend_from_slice(samples);

                // Update silence tracking
                if is_speech {
                    *silence_start = None;
                } else if silence_start.is_none() {
                    *silence_start = Some(Instant::now());
                }

                // Calculate current duration
                let duration_ms = speech_start.elapsed().as_millis() as u32;

                // Calculate required gap inline (to avoid borrow conflict with self.state)
                let required_gap = Self::calculate_required_gap_static(
                    duration_ms,
                    self.config.target_chunk_ms,
                    self.config.initial_gap_ms,
                    self.config.min_gap_ms,
                );

                // Check if should emit
                let should_emit = if duration_ms >= self.config.max_chunk_ms {
                    // Force emit at max duration
                    true
                } else {
                    // Check gap threshold
                    current_silence_ms >= required_gap
                };

                if should_emit {
                    // Emit chunk and reset to idle
                    let chunk = std::mem::take(buffer);
                    self.state = ChunkerState::Idle;
                    Some(chunk)
                } else {
                    None
                }
            }
        }
    }

    /// Force emit any accumulated audio (for shutdown).
    pub fn flush(&mut self) -> Option<Vec<i16>> {
        match &mut self.state {
            ChunkerState::Idle => None,
            ChunkerState::Accumulating { samples, .. } => {
                let chunk = std::mem::take(samples);
                self.state = ChunkerState::Idle;
                Some(chunk)
            }
        }
    }

    /// Calculate the required silence gap for a given speech duration.
    /// This is the core algorithm - linear interpolation between defined points.
    ///
    /// Gap threshold formula (based on speech research):
    /// - At 2500ms of speech: require 400ms silence gap (sentence boundaries)
    /// - At 3000ms: require 250ms gap (clause boundaries)
    /// - At 3500ms: require 150ms gap (inter-word gaps)
    /// - At 4000ms: require 100ms gap (safe minimum)
    /// - At 4500ms+: require 80ms gap (floor - never lower, to avoid mid-word cuts)
    pub fn required_gap_ms(&self, speech_duration_ms: u32) -> u32 {
        Self::calculate_required_gap_static(
            speech_duration_ms,
            self.config.target_chunk_ms,
            self.config.initial_gap_ms,
            self.config.min_gap_ms,
        )
    }

    /// Static version of required_gap calculation to avoid borrow conflicts.
    fn calculate_required_gap_static(
        speech_duration_ms: u32,
        target_chunk_ms: u32,
        initial_gap_ms: u32,
        min_gap_ms: u32,
    ) -> u32 {
        // Define interpolation points (duration_ms, gap_ms)
        const POINTS: [(u32, u32); 5] = [
            (2500, 400),
            (3000, 250),
            (3500, 150),
            (4000, 100),
            (4500, 80),
        ];

        // Below target: not actively looking for gaps yet
        if speech_duration_ms < target_chunk_ms {
            return initial_gap_ms;
        }

        // Find the two points to interpolate between
        for i in 0..POINTS.len() - 1 {
            let (d1, g1) = POINTS[i];
            let (d2, g2) = POINTS[i + 1];

            if speech_duration_ms <= d2 {
                // Linear interpolation between points
                if speech_duration_ms <= d1 {
                    return g1;
                }

                let duration_range = d2 - d1;
                let gap_range = g1 as i32 - g2 as i32; // Can be negative
                let progress = speech_duration_ms - d1;

                let interpolated_gap =
                    g1 as i32 - (gap_range * progress as i32) / duration_range as i32;
                return interpolated_gap.max(min_gap_ms as i32) as u32;
            }
        }

        // Beyond last point: use minimum gap (floor)
        min_gap_ms
    }

    /// Reset to idle state.
    pub fn reset(&mut self) {
        self.state = ChunkerState::Idle;
    }

    /// Get current accumulated duration in ms (for testing/debugging).
    pub fn accumulated_duration_ms(&self) -> u32 {
        match &self.state {
            ChunkerState::Idle => 0,
            ChunkerState::Accumulating {
                samples,
                speech_start: _,
                silence_start: _,
            } => {
                let num_samples = samples.len() as u32;
                (num_samples * 1000) / self.config.sample_rate
            }
        }
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

    #[test]
    fn test_required_gap_at_target() {
        let config = make_test_config();
        let chunker = AdaptiveChunker::new(config);

        // At exactly 2500ms, gap should be 400ms
        assert_eq!(chunker.required_gap_ms(2500), 400);
    }

    #[test]
    fn test_required_gap_interpolation() {
        let config = make_test_config();
        let chunker = AdaptiveChunker::new(config);

        // At 3250ms (halfway between 3000 and 3500), gap should be ~200ms
        // 3000ms -> 250ms, 3500ms -> 150ms
        // Midpoint: (250 + 150) / 2 = 200ms
        let gap = chunker.required_gap_ms(3250);
        assert!((195..=205).contains(&gap), "Expected ~200ms, got {}", gap);
    }

    #[test]
    fn test_required_gap_at_floor() {
        let config = make_test_config();
        let chunker = AdaptiveChunker::new(config);

        // At 5000ms (well past 4500ms), gap should be at floor (80ms)
        assert_eq!(chunker.required_gap_ms(5000), 80);
    }

    #[test]
    fn test_accumulates_during_speech() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        let samples = vec![1, 2, 3, 4, 5];

        // First speech frame
        let result = chunker.feed(true, &samples, 0);
        assert!(result.is_none());

        // Second speech frame
        let result = chunker.feed(true, &samples, 0);
        assert!(result.is_none());

        // Should have accumulated 10 samples (5 + 5)
        // Duration calculation: 10 samples * 1000ms / 16000 samples/sec = 0.625ms
        // But since it's less than 1ms, expect 0
        assert_eq!(chunker.accumulated_duration_ms(), 0);
    }

    #[test]
    fn test_emits_on_gap_threshold() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        // Generate enough samples to exceed target duration
        let samples_per_feed = 16000; // 1 second of audio at 16kHz
        let samples: Vec<i16> = (0..samples_per_feed).map(|i| i as i16).collect();

        // Feed speech for 3 seconds
        for _ in 0..3 {
            let result = chunker.feed(true, &samples, 0);
            assert!(result.is_none());
            std::thread::sleep(std::time::Duration::from_millis(1001));
        }

        // At 3000ms, required gap is 250ms
        // Feed silence with 250ms gap - should emit
        let result = chunker.feed(false, &samples, 250);
        assert!(result.is_some());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn test_emits_on_max_duration() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        let samples_per_feed = 16000; // 1 second
        let samples: Vec<i16> = (0..samples_per_feed).map(|i| i as i16).collect();

        // Feed speech for max duration (6 seconds)
        for i in 0..6 {
            let result = chunker.feed(true, &samples, 0);
            if i < 5 {
                assert!(result.is_none(), "Should not emit before max duration");
            }
            std::thread::sleep(std::time::Duration::from_millis(1001));
        }

        // Next feed should force emit (at max duration)
        let result = chunker.feed(true, &samples, 0);
        assert!(result.is_some(), "Should force emit at max duration");
    }

    #[test]
    fn test_ignores_silence_before_speech() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        let samples = vec![0, 0, 0, 0, 0];

        // Feed silence - should not accumulate
        let result = chunker.feed(false, &samples, 100);
        assert!(result.is_none());
        assert_eq!(chunker.accumulated_duration_ms(), 0);
    }

    #[test]
    fn test_flush_returns_accumulated() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        let samples = vec![1, 2, 3, 4, 5];

        // Feed some speech
        chunker.feed(true, &samples, 0);
        chunker.feed(true, &samples, 0);

        // Flush should return accumulated samples
        let result = chunker.flush();
        assert!(result.is_some());
        let chunk = result.unwrap();
        assert_eq!(chunk.len(), 10); // 2 feeds of 5 samples each

        // After flush, should be idle
        assert_eq!(chunker.accumulated_duration_ms(), 0);
    }

    #[test]
    fn test_reset_clears_state() {
        let config = make_test_config();
        let mut chunker = AdaptiveChunker::new(config);

        let samples = vec![1, 2, 3, 4, 5];

        // Feed some speech
        chunker.feed(true, &samples, 0);

        // Reset
        chunker.reset();

        // Should be idle, no accumulated samples
        assert_eq!(chunker.accumulated_duration_ms(), 0);

        // Flush should return nothing
        let result = chunker.flush();
        assert!(result.is_none());
    }
}
