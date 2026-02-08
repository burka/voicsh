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
    /// Duration of the audio content.
    pub audio_duration: Duration,
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

    /// Calculate the delay the user perceives after they stop speaking.
    /// This is total latency minus the time they were actually talking.
    pub fn perceived_wait(&self) -> Duration {
        self.total_latency().saturating_sub(self.audio_duration)
    }

    /// Calculate the real-time factor for transcription.
    /// < 1.0 means faster than real-time, > 1.0 means slower.
    pub fn realtime_factor(&self) -> f64 {
        if self.audio_duration.is_zero() {
            return 0.0;
        }
        self.transcription_latency().as_secs_f64() / self.audio_duration.as_secs_f64()
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
    pub audio_duration_avg: Duration,
    pub perceived_wait_avg: Duration,
    pub perceived_wait_min: Duration,
    pub perceived_wait_max: Duration,
    pub realtime_factor_avg: f64,
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
        let audio_durations: Vec<Duration> =
            self.measurements.iter().map(|t| t.audio_duration).collect();
        let perceived_waits: Vec<Duration> = self
            .measurements
            .iter()
            .map(|t| t.perceived_wait())
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
        let &perceived_wait_min = perceived_waits.iter().min()?;
        let &perceived_wait_max = perceived_waits.iter().max()?;

        let realtime_factor_sum: f64 = self.measurements.iter().map(|t| t.realtime_factor()).sum();
        let realtime_factor_avg = realtime_factor_sum / self.measurements.len() as f64;

        Some(LatencyStats {
            count: self.measurements.len(),
            total_avg: avg_duration(&total_latencies),
            total_min,
            total_max,
            vad_avg: avg_duration(&vad_latencies),
            chunking_avg: avg_duration(&chunking_latencies),
            transcription_avg: avg_duration(&transcription_latencies),
            output_avg: avg_duration(&output_latencies),
            audio_duration_avg: avg_duration(&audio_durations),
            perceived_wait_avg: avg_duration(&perceived_waits),
            perceived_wait_min,
            perceived_wait_max,
            realtime_factor_avg,
        })
    }

    /// Prints a user-friendly summary of session performance.
    pub fn print_summary(&self) {
        if let Some(stats) = self.stats() {
            eprintln!();
            eprintln!("=== Session Summary ===");
            eprintln!(
                "Transcribed {} utterance{}",
                stats.count,
                if stats.count == 1 { "" } else { "s" }
            );
            eprintln!();
            eprintln!(
                "  Avg spoken audio:         {}",
                format_duration(stats.audio_duration_avg)
            );
            eprintln!(
                "  Avg wait after speaking:  {}",
                format_duration(stats.perceived_wait_avg)
            );
            eprintln!(
                "    Transcription:          {}  ({:.1}x real-time)",
                format_duration(stats.transcription_avg),
                stats.realtime_factor_avg
            );
            eprintln!(
                "    Text output:            {}",
                format_duration(stats.output_avg)
            );
            eprintln!();
            eprintln!(
                "  Fastest: {} | Slowest: {}",
                format_duration(stats.perceived_wait_min),
                format_duration(stats.perceived_wait_max)
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
        let audio_secs = timing.audio_duration.as_secs_f64();
        let wait = format_duration(timing.perceived_wait());
        let transcribe = format_duration(timing.transcription_latency());
        let output = format_duration(timing.output_latency());
        eprintln!(
            "[{}] \"{text}\" — {audio_secs:.1}s audio, {wait} wait (transcribe {transcribe} {:.1}x, output {output})",
            transcription_num,
            timing.realtime_factor(),
        );
    }

    /// Prints basic timing for a single transcription.
    pub fn print_basic(&self, timing: &TranscriptionTiming, text: &str) {
        eprintln!(
            "\"{}\" ({} wait)",
            text,
            format_duration(timing.perceived_wait())
        );
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats a duration as a human-friendly string.
/// Under 1s: "450ms", at or above 1s: "1.5s".
fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", d.as_secs_f64())
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
            audio_duration: Duration::from_millis(15),
        };

        assert!(timing.total_latency().as_millis() >= 40);
        assert!(timing.vad_latency().as_millis() >= 10);
        assert!(timing.chunking_latency().as_millis() >= 5);
        assert!(timing.transcription_latency().as_millis() >= 20);
        assert!(timing.output_latency().as_millis() >= 5);
        // perceived_wait = total - audio_duration; total >= 40ms, audio = 15ms
        assert!(timing.perceived_wait().as_millis() >= 25);
        // realtime_factor = transcription / audio; transcription >= 20ms, audio = 15ms
        assert!(timing.realtime_factor() >= 1.0);
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
            audio_duration: Duration::from_millis(15),
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
        assert_eq!(stats.audio_duration_avg.as_millis(), 15);
        // perceived_wait = 110 - 15 = 95ms
        assert_eq!(stats.perceived_wait_avg.as_millis(), 95);
        assert_eq!(stats.perceived_wait_min.as_millis(), 95);
        assert_eq!(stats.perceived_wait_max.as_millis(), 95);
        // realtime_factor = 80ms / 15ms ≈ 5.3
        assert!(stats.realtime_factor_avg > 5.0);
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
            audio_duration: Duration::from_millis(10),
        };

        let timing2 = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(10),
            chunk_created: now + Duration::from_millis(25),
            transcription_done: now + Duration::from_millis(150),
            output_done: now + Duration::from_millis(165),
            audio_duration: Duration::from_millis(20),
        };

        tracker.record(timing1);
        tracker.record(timing2);

        let stats = tracker.stats().unwrap();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.total_min.as_millis(), 110);
        assert_eq!(stats.total_max.as_millis(), 165);
        // perceived_wait: timing1 = 110-10=100, timing2 = 165-20=145
        assert_eq!(stats.perceived_wait_min.as_millis(), 100);
        assert_eq!(stats.perceived_wait_max.as_millis(), 145);
    }

    #[test]
    fn test_format_duration_millis() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0ms");
        assert_eq!(format_duration(Duration::from_millis(450)), "450ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_millis(3750)), "3.8s");
    }

    #[test]
    fn test_perceived_wait_saturates_at_zero() {
        let now = Instant::now();
        let timing = TranscriptionTiming {
            capture_start: now,
            vad_start: now,
            chunk_created: now,
            transcription_done: now,
            output_done: now,
            // Audio duration longer than total latency
            audio_duration: Duration::from_secs(10),
        };
        assert_eq!(timing.perceived_wait(), Duration::ZERO);
    }

    #[test]
    fn test_realtime_factor_zero_audio() {
        let now = Instant::now();
        let timing = TranscriptionTiming {
            capture_start: now,
            vad_start: now,
            chunk_created: now,
            transcription_done: now + Duration::from_millis(100),
            output_done: now + Duration::from_millis(100),
            audio_duration: Duration::ZERO,
        };
        assert_eq!(timing.realtime_factor(), 0.0);
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
        // Exercises all three print paths to ensure no panics from formatting.
        let mut tracker = LatencyTracker::new();
        let now = Instant::now();
        let timing = TranscriptionTiming {
            capture_start: now,
            vad_start: now + Duration::from_millis(10),
            chunk_created: now + Duration::from_millis(20),
            transcription_done: now + Duration::from_millis(100),
            output_done: now + Duration::from_millis(110),
            audio_duration: Duration::from_millis(15),
        };

        tracker.record(timing.clone());
        tracker.print_summary();
        tracker.print_detailed(&timing, "test text", 1);
        tracker.print_basic(&timing, "test text");
    }
}
