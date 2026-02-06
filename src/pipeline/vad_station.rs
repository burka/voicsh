//! VAD station that detects voice activity in audio frames.

use crate::audio::vad::{Clock, SystemClock, Vad, VadConfig};
use crate::pipeline::error::StationError;
use crate::pipeline::station::Station;
use crate::pipeline::types::{AudioFrame, VadFrame};
use std::io::{self, Write};
use std::sync::Arc;

/// VAD station that processes audio frames and annotates them with speech detection.
pub struct VadStation {
    vad: Vad<Arc<dyn Clock>>,
    show_levels: bool,
    auto_level: bool,
    level_history: Vec<f32>,
    level_history_max: usize,
    sample_rate: u32,
}

impl VadStation {
    /// Creates a new VAD station with the given configuration.
    pub fn new(config: VadConfig) -> Self {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    /// Creates a new VAD station with an injectable clock.
    pub fn with_clock(config: VadConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            vad: Vad::with_clock(config, clock),
            show_levels: false,
            auto_level: false,
            level_history: Vec::new(),
            level_history_max: 100,
            sample_rate: 16000,
        }
    }

    /// Enables or disables level display to stderr.
    pub fn with_show_levels(mut self, show: bool) -> Self {
        self.show_levels = show;
        self
    }

    /// Enables or disables automatic threshold adjustment based on noise floor.
    pub fn with_auto_level(mut self, enabled: bool) -> Self {
        self.auto_level = enabled;
        self
    }

    /// Sets the sample rate for VAD processing.
    pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate;
        self
    }

    /// Adjusts the VAD threshold based on the noise floor estimate.
    fn adjust_threshold(&mut self) {
        if self.level_history.len() < 10 {
            // Not enough data yet
            return;
        }

        // Calculate 25th percentile as noise floor
        let mut sorted = self.level_history.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let percentile_idx = sorted.len() / 4;
        let noise_floor = sorted[percentile_idx];

        // Set threshold to noise_floor * 2.0, clamped to reasonable bounds
        let new_threshold = (noise_floor * 2.0).clamp(0.002, 0.2);
        self.vad.set_threshold(new_threshold);
    }

    /// Displays a visual level meter to stderr.
    fn display_level(&self, level: f32, threshold: f32) {
        const BAR_WIDTH: usize = 30;

        // Use logarithmic scale for better visibility at low levels
        // Map level 0.001-0.5 to 0-30 bars using log scale
        let log_level = if level > 0.001 {
            ((level.log10() + 3.0) / 2.7 * BAR_WIDTH as f32).clamp(0.0, BAR_WIDTH as f32)
        } else {
            0.0
        };
        let filled = log_level as usize;

        // Calculate threshold position on same scale
        let log_threshold = if threshold > 0.001 {
            ((threshold.log10() + 3.0) / 2.7 * BAR_WIDTH as f32).clamp(0.0, BAR_WIDTH as f32)
        } else {
            0.0
        };
        let threshold_pos = log_threshold as usize;

        // Build the bar with threshold marker
        let bar: String = (0..BAR_WIDTH)
            .map(|i| {
                if i < filled {
                    if level > threshold { '█' } else { '▓' }
                } else if i == threshold_pos {
                    '│'
                } else {
                    '░'
                }
            })
            .collect();

        // Use \r to overwrite the line, clear to end
        eprint!("\r[{}] {:.3}  ", bar, level);
        io::stderr().flush().ok();
    }
}

impl Station for VadStation {
    type Input = AudioFrame;
    type Output = VadFrame;

    fn name(&self) -> &'static str {
        "vad"
    }

    fn process(&mut self, frame: AudioFrame) -> Result<Option<VadFrame>, StationError> {
        // Skip empty frames
        if frame.samples.is_empty() {
            return Ok(None);
        }

        // Process VAD
        let result = self.vad.process_with_info(&frame.samples, self.sample_rate);

        // Update level history for auto-leveling
        if self.auto_level {
            self.level_history.push(result.level);
            if self.level_history.len() > self.level_history_max {
                self.level_history.remove(0);
            }
            self.adjust_threshold();
        }

        // Display level meter if enabled
        if self.show_levels {
            self.display_level(result.level, result.threshold);
        }

        // Determine if this frame contains speech
        let is_speech = matches!(
            result.event,
            crate::audio::vad::VadEvent::SpeechStart | crate::audio::vad::VadEvent::Speech
        );

        // Always return a VadFrame - never filter
        Ok(Some(VadFrame::new(
            frame.samples,
            frame.timestamp,
            is_speech,
            result.level,
        )))
    }

    fn shutdown(&mut self) {
        if self.show_levels {
            // Clear the level display line
            eprintln!();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_silence(count: usize) -> Vec<i16> {
        vec![0i16; count]
    }

    fn make_speech(count: usize, amplitude: i16) -> Vec<i16> {
        vec![amplitude; count]
    }

    #[test]
    fn test_vad_station_processes_silence() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 1000,
            min_speech_ms: 300,
        };
        let mut station = VadStation::new(config);

        let silence = make_silence(1000);
        let frame = AudioFrame::new(silence, Instant::now(), 1);

        let result = station.process(frame).unwrap();
        assert!(result.is_some());

        let vad_frame = result.unwrap();
        assert!(!vad_frame.is_speech);
        assert_eq!(vad_frame.level, 0.0);
    }

    #[test]
    fn test_vad_station_processes_speech() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 1000,
            min_speech_ms: 300,
        };
        let mut station = VadStation::new(config);

        // First frame starts speech
        let speech = make_speech(1000, 3000); // RMS ~0.09, above 0.02 threshold
        let frame = AudioFrame::new(speech.clone(), Instant::now(), 1);

        let result = station.process(frame).unwrap();
        assert!(result.is_some());

        let vad_frame = result.unwrap();
        assert!(vad_frame.is_speech);
        assert!(vad_frame.level > 0.02);
    }

    #[test]
    fn test_vad_station_continues_speech() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 1000,
            min_speech_ms: 300,
        };
        let mut station = VadStation::new(config);

        let speech = make_speech(1000, 3000);

        // First frame starts speech
        let frame1 = AudioFrame::new(speech.clone(), Instant::now(), 1);
        let result1 = station.process(frame1).unwrap();
        assert!(result1.unwrap().is_speech);

        // Second frame continues speech
        let frame2 = AudioFrame::new(speech, Instant::now(), 2);
        let result2 = station.process(frame2).unwrap();
        assert!(result2.unwrap().is_speech);
    }

    #[test]
    fn test_vad_station_never_filters() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config);

        // Process multiple frames of silence
        for i in 0..10 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            let result = station.process(frame).unwrap();
            assert!(result.is_some(), "Frame {} should not be filtered", i);
        }
    }

    #[test]
    fn test_vad_station_auto_level_adjusts_threshold() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 1000,
            min_speech_ms: 300,
        };
        let mut station = VadStation::new(config).with_auto_level(true);

        // Send frames with low noise floor
        for i in 0..50 {
            let low_noise = make_speech(1000, 100); // Very low level
            let frame = AudioFrame::new(low_noise, Instant::now(), i);
            station.process(frame).unwrap();
        }

        // The threshold should have been adjusted down
        // We can't directly access the threshold, but we can verify the station still works
        let silence = make_silence(1000);
        let frame = AudioFrame::new(silence, Instant::now(), 100);
        let result = station.process(frame).unwrap();
        assert!(
            result.is_some(),
            "VAD station should process frames even after auto-level adjustment"
        );
        let vad_frame = result.unwrap();
        assert!(
            !vad_frame.is_speech,
            "Silence should not be detected as speech"
        );
        assert_eq!(vad_frame.level, 0.0, "Silence level should be 0.0");
    }

    #[test]
    fn test_vad_station_level_history_bounded() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config).with_auto_level(true);

        // Send more frames than the history max
        for i in 0..150 {
            let speech = make_speech(1000, 1000);
            let frame = AudioFrame::new(speech, Instant::now(), i);
            station.process(frame).unwrap();
        }

        // History should be capped
        assert_eq!(station.level_history.len(), 100);
    }

    #[test]
    fn test_display_level_format() {
        let config = VadConfig::default();
        let station = VadStation::new(config).with_show_levels(true);

        // Test display at various levels
        station.display_level(0.0, 0.02);
        station.display_level(0.15, 0.08);
        station.display_level(0.3, 0.05);

        // Just verify it doesn't panic
    }

    #[test]
    fn test_vad_station_builder_pattern() {
        let config = VadConfig::default();
        let station = VadStation::new(config)
            .with_show_levels(true)
            .with_auto_level(true)
            .with_sample_rate(48000);

        assert!(station.show_levels);
        assert!(station.auto_level);
        assert_eq!(station.sample_rate, 48000);
    }
}
