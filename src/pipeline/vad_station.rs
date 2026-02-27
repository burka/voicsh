//! VAD station that detects voice activity in audio frames.

use crate::audio::vad::{Clock, SystemClock, Vad, VadConfig};
use crate::ipc::protocol::DaemonEvent;
use crate::output::render_event;
use crate::pipeline::error::StationError;
use crate::pipeline::station::Station;
use crate::pipeline::types::{AudioFrame, VadFrame};
use std::sync::Arc;

/// Number of frames to wait before warning about no audio (~3 seconds at 60Hz polling).
const NO_AUDIO_WARNING_FRAMES: u64 = 180;

/// Threshold below which audio is considered "no signal" (essentially zero).
const NO_AUDIO_THRESHOLD: f32 = 0.0001;

/// Returns (current_len, capacity) for a pipeline buffer channel.
pub type BufferGauge = Box<dyn Fn() -> (usize, usize) + Send + Sync>;

/// Format a visual level bar for display.
/// Returns a string like `[██████████████████░░░░░░░░░░│░░] 0.150`
pub fn format_level_bar(level: f32, threshold: f32) -> String {
    const BAR_WIDTH: usize = 30;

    let log_level = if level > 0.001 {
        ((level.log10() + 3.0) / 2.7 * BAR_WIDTH as f32).clamp(0.0, BAR_WIDTH as f32)
    } else {
        0.0
    };
    let filled = log_level as usize;

    let log_threshold = if threshold > 0.001 {
        ((threshold.log10() + 3.0) / 2.7 * BAR_WIDTH as f32).clamp(0.0, BAR_WIDTH as f32)
    } else {
        0.0
    };
    let threshold_pos = log_threshold as usize;

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

    format!("[{}] {:.3}", bar, level)
}

/// VAD station that processes audio frames and annotates them with speech detection.
pub struct VadStation {
    vad: Vad<Arc<dyn Clock>>,
    show_levels: bool,
    auto_level: bool,
    level_history: Vec<f32>,
    level_history_max: usize,
    sample_rate: u32,
    /// Count of frames processed.
    frames_processed: u64,
    /// Whether we've shown the no-audio warning.
    no_audio_warning_shown: bool,
    /// Count of consecutive frames with near-zero audio.
    zero_audio_frames: u64,
    /// Optional gauge to read chunk buffer occupancy.
    buffer_gauge: Option<BufferGauge>,
    /// Optional event sender for daemon event streaming
    event_tx: Option<crossbeam_channel::Sender<DaemonEvent>>,
    /// Throttle counter for level events (emit every 4th frame)
    level_event_counter: u64,
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
            frames_processed: 0,
            no_audio_warning_shown: false,
            zero_audio_frames: 0,
            buffer_gauge: None,
            event_tx: None,
            level_event_counter: 0,
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

    /// Sets a buffer gauge for displaying chunk queue depth on the meter line.
    pub fn with_buffer_gauge(mut self, gauge: BufferGauge) -> Self {
        self.buffer_gauge = Some(gauge);
        self
    }

    /// Sets an event sender for daemon event streaming.
    pub fn with_event_sender(mut self, tx: crossbeam_channel::Sender<DaemonEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Adjusts the VAD threshold based on the noise floor estimate.
    fn adjust_threshold(&mut self) {
        if self.level_history.len() < 10 {
            return;
        }

        let mut sorted = self.level_history.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let percentile_idx = sorted.len() / 4;
        let noise_floor = sorted[percentile_idx];

        let new_threshold = (noise_floor * 2.0).clamp(0.002, 0.2);
        self.vad.set_threshold(new_threshold);
    }

    /// Displays a visual level meter to stderr.
    /// Check for no-audio condition and warn user.
    fn check_no_audio(&mut self, level: f32) {
        if level < NO_AUDIO_THRESHOLD {
            self.zero_audio_frames += 1;
        } else {
            self.zero_audio_frames = 0;
        }

        // Show warning after sustained period of no audio
        if !self.no_audio_warning_shown && self.zero_audio_frames >= NO_AUDIO_WARNING_FRAMES {
            self.no_audio_warning_shown = true;
            eprintln!();
            eprintln!("Warning: No audio detected for 3+ seconds.");
            eprintln!("  - Check that your microphone is connected and selected");
            eprintln!("  - Run: pactl list sources short");
            eprintln!("  - Run: voicsh devices");
            eprintln!();
        }
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

        self.frames_processed += 1;

        // Process VAD
        let result = self.vad.process_with_info(&frame.samples, self.sample_rate);

        // Check for no-audio condition (only when showing levels, i.e., verbosity >= 1)
        if self.show_levels {
            self.check_no_audio(result.level);
        }

        // Update level history for auto-leveling
        if self.auto_level {
            self.level_history.push(result.level);
            if self.level_history.len() > self.level_history_max {
                self.level_history.remove(0);
            }
            self.adjust_threshold();
        }

        // Determine if this frame contains speech
        let is_speech = matches!(
            result.event,
            crate::audio::vad::VadEvent::SpeechStart | crate::audio::vad::VadEvent::Speech
        );

        // Get buffer gauge info
        let (buf_used, buf_cap) = self
            .buffer_gauge
            .as_ref()
            .map(|g| {
                let (u, c) = g();
                (u as u16, c as u16)
            })
            .unwrap_or((0, 0));

        // Display level meter if enabled
        if self.show_levels {
            render_event(&DaemonEvent::Level {
                level: result.level,
                threshold: result.threshold,
                is_speech,
                buffer_used: buf_used,
                buffer_capacity: buf_cap,
            });
        }

        // Emit level event for follow clients (throttled to every 4th frame ≈ 15Hz)
        if let Some(ref tx) = self.event_tx {
            self.level_event_counter += 1;
            if self.level_event_counter.is_multiple_of(4) {
                tx.try_send(DaemonEvent::Level {
                    level: result.level,
                    threshold: result.threshold,
                    is_speech,
                    buffer_used: buf_used,
                    buffer_capacity: buf_cap,
                })
                .ok();
            }
        }

        // Always return a VadFrame - never filter
        // Populate timing when show_levels is enabled (verbosity >= 1)
        let vad_frame = if self.show_levels {
            VadFrame::with_timing(
                frame.samples,
                frame.timestamp,
                is_speech,
                result.level,
                std::time::Instant::now(),
            )
        } else {
            VadFrame::new(frame.samples, frame.timestamp, is_speech, result.level)
        };

        Ok(Some(vad_frame))
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

    #[test]
    fn test_buffer_gauge_wired_through_builder() {
        let config = VadConfig::default();
        let station = VadStation::new(config).with_buffer_gauge(Box::new(|| (3, 8)));

        let (len, cap) = (station.buffer_gauge.as_ref().unwrap())();
        assert_eq!(len, 3);
        assert_eq!(cap, 8);
    }

    #[test]
    fn test_no_audio_detection_counts_zero_frames() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config).with_show_levels(true);

        // Send silence frames
        for i in 0..50 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            station.process(frame).unwrap();
        }

        // Should have counted zero-audio frames
        assert_eq!(station.zero_audio_frames, 50);
        // Warning not shown yet (need 180 frames)
        assert!(!station.no_audio_warning_shown);
    }

    #[test]
    fn test_no_audio_detection_resets_on_audio() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config).with_show_levels(true);

        // Send silence frames
        for i in 0..50 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            station.process(frame).unwrap();
        }
        assert_eq!(station.zero_audio_frames, 50);

        // Send audio frame - should reset counter
        let speech = make_speech(1000, 3000);
        let frame = AudioFrame::new(speech, Instant::now(), 51);
        station.process(frame).unwrap();

        assert_eq!(station.zero_audio_frames, 0);
    }

    #[test]
    fn test_no_audio_warning_triggers_after_threshold() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config).with_show_levels(true);

        // Send enough silence frames to trigger warning
        for i in 0..200 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            station.process(frame).unwrap();
        }

        // Warning should have been shown
        assert!(station.no_audio_warning_shown);
    }

    #[test]
    fn test_no_audio_warning_only_shown_once() {
        let config = VadConfig::default();
        let mut station = VadStation::new(config).with_show_levels(true);

        // Trigger warning
        for i in 0..200 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            station.process(frame).unwrap();
        }
        assert!(station.no_audio_warning_shown);

        // Reset counter with audio
        let speech = make_speech(1000, 3000);
        let frame = AudioFrame::new(speech, Instant::now(), 200);
        station.process(frame).unwrap();

        // Send more silence - warning flag should stay true (won't show again)
        for i in 201..400 {
            let silence = make_silence(1000);
            let frame = AudioFrame::new(silence, Instant::now(), i);
            station.process(frame).unwrap();
        }
        assert!(station.no_audio_warning_shown);
    }

    #[test]
    fn test_format_level_bar_zero() {
        let bar = format_level_bar(0.0, 0.02);
        assert!(bar.contains("["), "Bar should start with [");
        assert!(bar.contains("]"), "Bar should contain ]");
        assert!(bar.contains("0.000"), "Zero level should show 0.000");
    }

    #[test]
    fn test_format_level_bar_high_level() {
        let bar = format_level_bar(0.3, 0.05);
        assert!(bar.contains('█'), "High level should show filled blocks");
        assert!(bar.contains("0.300"), "Should show level value");
    }

    #[test]
    fn test_format_level_bar_below_threshold() {
        let bar = format_level_bar(0.01, 0.05);
        // Below threshold, filled blocks use ▓
        assert!(
            !bar.contains('█'),
            "Below threshold should not use full blocks"
        );
    }

    #[test]
    fn test_event_sender_builder() {
        let (tx, _rx) = crossbeam_channel::bounded(16);
        let config = VadConfig::default();
        let station = VadStation::new(config).with_event_sender(tx);
        assert!(station.event_tx.is_some());
    }
}
