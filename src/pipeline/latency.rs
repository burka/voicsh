//! Latency measurement and reporting for the audio pipeline.

use std::time::{Duration, Instant};

/// Timing information for a single transcription.
#[derive(Debug, Clone)]
pub struct TranscriptionTiming {
    /// When the first audio frame was captured.
    pub capture_start: Instant,
    /// When VAD processing began.
    pub vad_start: Instant,
    /// When the chunk was created and sent to transcription.
    pub chunk_created: Instant,
    /// When transcription completed.
    pub transcription_done: Instant,
    /// When text output/injection completed.
    pub output_done: Instant,
}

impl TranscriptionTiming {
    /// Calculate total end-to-end latency.
    pub fn total_latency(&self) -> Duration {
        self.output_done.duration_since(self.capture_start)
    }

    /// Calculate time spent in VAD processing.
    pub fn vad_latency(&self) -> Duration {
        self.vad_start.duration_since(self.capture_start)
    }

    /// Calculate time spent in chunking/buffering.
    pub fn chunking_latency(&self) -> Duration {
        self.chunk_created.duration_since(self.vad_start)
    }

    /// Calculate time spent in transcription.
    pub fn transcription_latency(&self) -> Duration {
        self.transcription_done.duration_since(self.chunk_created)
    }

    /// Calculate time spent in output/injection.
    pub fn output_latency(&self) -> Duration {
        self.output_done.duration_since(self.transcription_done)
    }
}

/// Aggregated latency statistics.
#[derive(Debug, Clone)]
pub struct LatencyStats {
    pub count: usize,
    pub total_avg: Duration,
    pub total_min: Duration,
    pub total_max: Duration,
    pub vad_avg: Duration,
    pub chunking_avg: Duration,
    pub transcription_avg: Duration,
    pub output_avg: Duration,
}

/// Collects and reports latency measurements.
pub struct LatencyTracker {
    measurements: Vec<TranscriptionTiming>,
}

impl LatencyTracker {
    /// Creates a new latency tracker.
    pub fn new() -> Self {
        Self {
            measurements: Vec::new(),
        }
    }

    /// Records a timing measurement.
    pub fn record(&mut self, timing: TranscriptionTiming) {
        self.measurements.push(timing);
    }

    /// Computes aggregated statistics.
    pub fn stats(&self) -> Option<LatencyStats> {
        if self.measurements.is_empty() {
            return None;
        }

        let total_latencies: Vec<Duration> = self
            .measurements
            .iter()
            .map(|t| t.total_latency())
            .collect();
        let vad_latencies: Vec<Duration> =
            self.measurements.iter().map(|t| t.vad_latency()).collect();
        let chunking_latencies: Vec<Duration> = self
            .measurements
            .iter()
            .map(|t| t.chunking_latency())
            .collect();
        let transcription_latencies: Vec<Duration> = self
            .measurements
            .iter()
            .map(|t| t.transcription_latency())
            .collect();
        let output_latencies: Vec<Duration> = self
            .measurements
            .iter()
            .map(|t| t.output_latency())
            .collect();

        // We've already checked that measurements is not empty, so these will always succeed
        let Some(&total_min) = total_latencies.iter().min() else {
            // This branch is unreachable since we checked that measurements is not empty
            return None;
        };
        let Some(&total_max) = total_latencies.iter().max() else {
            // This branch is unreachable since we checked that measurements is not empty
            return None;
        };

        Some(LatencyStats {
            count: self.measurements.len(),
            total_avg: avg_duration(&total_latencies),
            total_min,
            total_max,
            vad_avg: avg_duration(&vad_latencies),
            chunking_avg: avg_duration(&chunking_latencies),
            transcription_avg: avg_duration(&transcription_latencies),
            output_avg: avg_duration(&output_latencies),
        })
    }

    /// Prints a summary of latency statistics.
    pub fn print_summary(&self) {
        if let Some(stats) = self.stats() {
            eprintln!();
            eprintln!("=== Latency Summary ===");
            eprintln!("Total transcriptions: {}", stats.count);
            eprintln!(
                "Average latency: {}ms (min: {}ms, max: {}ms)",
                stats.total_avg.as_millis(),
                stats.total_min.as_millis(),
                stats.total_max.as_millis()
            );
            eprintln!(
                "Average breakdown: Capture→VAD {}ms, VAD→Chunk {}ms, Transcription {}ms, Output {}ms",
                stats.vad_avg.as_millis(),
                stats.chunking_avg.as_millis(),
                stats.transcription_avg.as_millis(),
                stats.output_avg.as_millis()
            );
        }
    }

    /// Prints detailed timing for a single transcription.
    pub fn print_detailed(
        &self,
        timing: &TranscriptionTiming,
        text: &str,
        transcription_num: usize,
    ) {
        eprintln!(
            "[Transcription {}] \"{}\" ({}ms total: {}ms capture→VAD, {}ms chunking, {}ms transcription, {}ms output)",
            transcription_num,
            text,
            timing.total_latency().as_millis(),
            timing.vad_latency().as_millis(),
            timing.chunking_latency().as_millis(),
            timing.transcription_latency().as_millis(),
            timing.output_latency().as_millis()
        );
    }

    /// Prints basic timing for a single transcription.
    pub fn print_basic(&self, timing: &TranscriptionTiming, text: &str) {
        eprintln!("\"{}\" ({}ms)", text, timing.total_latency().as_millis());
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculates average duration from a slice.
fn avg_duration(durations: &[Duration]) -> Duration {
    if durations.is_empty() {
        return Duration::from_secs(0);
    }
    let sum: Duration = durations.iter().sum();
    sum / durations.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_transcription_timing_calculations() {
        let capture = Instant::now();
        thread::sleep(Duration::from_millis(10));
        let vad_start = Instant::now();
        thread::sleep(Duration::from_millis(5));
        let chunk_created = Instant::now();
        thread::sleep(Duration::from_millis(20));
        let transcription_done = Instant::now();
        thread::sleep(Duration::from_millis(5));
        let output_done = Instant::now();

        let timing = TranscriptionTiming {
            capture_start: capture,
            vad_start,
            chunk_created,
            transcription_done,
            output_done,
        };

        assert!(timing.total_latency().as_millis() >= 40);
        assert!(timing.vad_latency().as_millis() >= 10);
        assert!(timing.chunking_latency().as_millis() >= 5);
        assert!(timing.transcription_latency().as_millis() >= 20);
        assert!(timing.output_latency().as_millis() >= 5);
    }

    #[test]
    fn test_latency_tracker_empty() {
        let tracker = LatencyTracker::new();
        assert!(tracker.stats().is_none());
    }

    #[test]
    fn test_latency_tracker_single_measurement() {
        let mut tracker = LatencyTracker::new();
        let now = Instant::now();
        let timing = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(10),
            chunk_created: now + Duration::from_millis(20),
            transcription_done: now + Duration::from_millis(100),
            output_done: now + Duration::from_millis(110),
        };

        tracker.record(timing);

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 1);
        assert_eq!(stats.total_avg.as_millis(), 110);
        assert_eq!(stats.total_min.as_millis(), 110);
        assert_eq!(stats.total_max.as_millis(), 110);
        assert_eq!(stats.vad_avg.as_millis(), 10);
        assert_eq!(stats.chunking_avg.as_millis(), 10);
        assert_eq!(stats.transcription_avg.as_millis(), 80);
        assert_eq!(stats.output_avg.as_millis(), 10);
    }

    #[test]
    fn test_latency_tracker_multiple_measurements() {
        let mut tracker = LatencyTracker::new();
        let now = Instant::now();

        let timing1 = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(5),
            chunk_created: now + Duration::from_millis(15),
            transcription_done: now + Duration::from_millis(100),
            output_done: now + Duration::from_millis(110),
        };

        let timing2 = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(10),
            chunk_created: now + Duration::from_millis(25),
            transcription_done: now + Duration::from_millis(150),
            output_done: now + Duration::from_millis(165),
        };

        tracker.record(timing1);
        tracker.record(timing2);

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.total_min.as_millis(), 110);
        assert_eq!(stats.total_max.as_millis(), 165);
    }

    #[test]
    fn test_avg_duration_empty() {
        let durations: Vec<Duration> = vec![];
        let avg = avg_duration(&durations);
        assert_eq!(avg.as_millis(), 0);
    }

    #[test]
    fn test_avg_duration_single() {
        let durations = vec![Duration::from_millis(100)];
        let avg = avg_duration(&durations);
        assert_eq!(avg.as_millis(), 100);
    }

    #[test]
    fn test_avg_duration_multiple() {
        let durations = vec![
            Duration::from_millis(100),
            Duration::from_millis(200),
            Duration::from_millis(300),
        ];
        let avg = avg_duration(&durations);
        assert_eq!(avg.as_millis(), 200);
    }

    #[test]
    fn test_print_summary_doesnt_panic() {
        let mut tracker = LatencyTracker::new();
        let now = Instant::now();
        let timing = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(10),
            chunk_created: now + Duration::from_millis(20),
            transcription_done: now + Duration::from_millis(100),
            output_done: now + Duration::from_millis(110),
        };

        tracker.record(timing.clone());
        tracker.print_summary();
        tracker.print_detailed(&timing, "test text", 1);
        tracker.print_basic(&timing, "test text");
    }
}
