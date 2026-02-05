//! Silence detector station.
//!
//! Monitors audio stream and emits control frames:
//! - `SpeechStart` when speech begins
//! - `FlushChunk` when significant pause detected
//! - `SpeechEnd` when speech ends
//!
//! Also supports:
//! - Level meter display for debugging
//! - Auto-leveling (AGC) for varying input volumes

use crate::audio::vad::{Vad, VadConfig, VadEvent, VadResult};
use crate::defaults;
use crate::streaming::frame::{AudioFrame, ControlEvent, PipelineFrame};
use std::io::{self, Write};
use tokio::sync::mpsc;

/// Configuration for the silence detector.
#[derive(Debug, Clone)]
pub struct SilenceDetectorConfig {
    /// VAD configuration.
    pub vad: VadConfig,
    /// Minimum pause duration (ms) before emitting FlushChunk.
    pub flush_pause_ms: u32,
    /// Sample rate for duration calculations.
    pub sample_rate: u32,
    /// Enable auto-leveling (AGC).
    pub auto_level: bool,
    /// Show level meter output.
    pub show_levels: bool,
}

impl Default for SilenceDetectorConfig {
    fn default() -> Self {
        Self {
            vad: VadConfig::default(),
            flush_pause_ms: 500,
            sample_rate: defaults::SAMPLE_RATE,
            auto_level: true,
            show_levels: false,
        }
    }
}

/// Auto-leveling state for adaptive threshold adjustment.
#[derive(Debug)]
struct AutoLevel {
    /// Running average of ambient noise level.
    ambient_level: f32,
    /// Smoothing factor for ambient level (0-1, higher = more smoothing).
    smoothing: f32,
    /// Minimum threshold (never go below this).
    min_threshold: f32,
    /// Multiplier above ambient to set threshold.
    threshold_multiplier: f32,
    /// Number of samples processed.
    sample_count: u64,
}

impl AutoLevel {
    fn new() -> Self {
        Self {
            ambient_level: 0.01,
            smoothing: 0.95,
            min_threshold: 0.01,
            threshold_multiplier: 2.5,
            sample_count: 0,
        }
    }

    /// Update ambient level from a silence frame and return adjusted threshold.
    fn update(&mut self, level: f32, is_speech: bool) -> f32 {
        self.sample_count += 1;

        // Only update ambient level during non-speech periods
        if !is_speech && self.sample_count > 10 {
            // Use longer window for more stable ambient tracking
            let alpha = if self.sample_count < 100 {
                0.1 // Learn faster initially
            } else {
                1.0 - self.smoothing
            };
            self.ambient_level = self.ambient_level * (1.0 - alpha) + level * alpha;
        }

        // Calculate threshold as multiplier of ambient level
        (self.ambient_level * self.threshold_multiplier).max(self.min_threshold)
    }

    /// Get current ambient level estimate.
    fn ambient(&self) -> f32 {
        self.ambient_level
    }
}

/// Silence detector that wraps VAD and emits control frames.
pub struct SilenceDetectorStation {
    config: SilenceDetectorConfig,
    vad: Vad,
    speech_active: bool,
    flush_sent: bool,
    auto_level: Option<AutoLevel>,
    last_level: f32,
    last_threshold: f32,
}

impl SilenceDetectorStation {
    /// Creates a new silence detector with default configuration.
    pub fn new() -> Self {
        Self::with_config(SilenceDetectorConfig::default())
    }

    /// Creates a new silence detector with custom configuration.
    pub fn with_config(config: SilenceDetectorConfig) -> Self {
        let vad = Vad::new(config.vad);
        let auto_level = if config.auto_level {
            Some(AutoLevel::new())
        } else {
            None
        };
        Self {
            config,
            vad,
            speech_active: false,
            flush_sent: false,
            auto_level,
            last_level: 0.0,
            last_threshold: 0.02,
        }
    }

    /// Processes an audio frame and returns any control events that should be emitted.
    pub fn process(&mut self, frame: &AudioFrame) -> Option<ControlEvent> {
        let result = self
            .vad
            .process_with_info(&frame.samples, self.config.sample_rate);

        // Store for level display
        self.last_level = result.level;
        self.last_threshold = result.threshold;

        // Update auto-level if enabled
        if let Some(ref mut auto_level) = self.auto_level {
            let is_speech = matches!(result.event, VadEvent::Speech | VadEvent::SpeechStart);
            let new_threshold = auto_level.update(result.level, is_speech);

            // Update VAD threshold dynamically without resetting state
            self.vad.set_threshold(new_threshold);
            self.last_threshold = new_threshold;
        }

        // Display level meter if enabled
        if self.config.show_levels {
            self.display_level(&result);
        }

        self.process_vad_result(&result)
    }

    /// Display audio level as a visual meter.
    fn display_level(&self, result: &VadResult) {
        let bar_width = 20;
        let level_pct = (result.level / 0.1).min(1.0);
        let filled = (level_pct * bar_width as f32) as usize;
        let threshold_pos = ((self.last_threshold / 0.1).min(1.0) * bar_width as f32) as usize;

        let mut bar = String::with_capacity(bar_width);
        for i in 0..bar_width {
            if i < filled {
                if i >= threshold_pos {
                    bar.push('█'); // Above threshold
                } else {
                    bar.push('▒'); // Below threshold
                }
            } else if i == threshold_pos {
                bar.push('│'); // Threshold marker
            } else {
                bar.push('░'); // Empty
            }
        }

        let status = if self.speech_active {
            if result.silence_ms > 0 {
                format!("silence {:.1}s", result.silence_ms as f32 / 1000.0)
            } else {
                "recording".to_string()
            }
        } else {
            "waiting".to_string()
        };

        // Show ambient level if auto-leveling
        let ambient_info = if let Some(ref al) = self.auto_level {
            format!(" amb:{:.3}", al.ambient())
        } else {
            String::new()
        };

        eprint!("\r[{}] {:12}{} ", bar, status, ambient_info);
        let _ = io::stderr().flush();
    }

    /// Clear the level display line.
    fn clear_level_display(&self) {
        if self.config.show_levels {
            eprint!("\r{:60}\r", "");
            let _ = io::stderr().flush();
        }
    }

    /// Processes a VAD result and determines if a control event should be emitted.
    fn process_vad_result(&mut self, result: &VadResult) -> Option<ControlEvent> {
        match result.event {
            VadEvent::SpeechStart => {
                self.speech_active = true;
                self.flush_sent = false;
                Some(ControlEvent::SpeechStart)
            }
            VadEvent::Speech => {
                self.flush_sent = false;
                None
            }
            VadEvent::Silence if self.speech_active => {
                if !self.flush_sent && result.silence_ms >= self.config.flush_pause_ms {
                    self.flush_sent = true;
                    Some(ControlEvent::FlushChunk)
                } else {
                    None
                }
            }
            VadEvent::Silence => None,
            VadEvent::SpeechEnd => {
                self.speech_active = false;
                self.flush_sent = false;
                self.clear_level_display();
                Some(ControlEvent::SpeechEnd)
            }
        }
    }

    /// Returns true if speech is currently active.
    pub fn is_speech_active(&self) -> bool {
        self.speech_active
    }

    /// Resets the detector state.
    pub fn reset(&mut self) {
        self.vad.reset();
        self.speech_active = false;
        self.flush_sent = false;
        if let Some(ref mut al) = self.auto_level {
            *al = AutoLevel::new();
        }
    }

    /// Runs the silence detector as a station.
    pub async fn run(
        mut self,
        mut input: mpsc::Receiver<AudioFrame>,
        output: mpsc::Sender<PipelineFrame>,
    ) {
        while let Some(frame) = input.recv().await {
            if let Some(control) = self.process(&frame) {
                if output.send(PipelineFrame::Control(control)).await.is_err() {
                    break;
                }

                if control == ControlEvent::SpeechEnd {
                    break;
                }
            }

            if output.send(PipelineFrame::Audio(frame)).await.is_err() {
                break;
            }
        }

        self.clear_level_display();
    }
}

impl Default for SilenceDetectorStation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_silence(count: usize) -> Vec<i16> {
        vec![0i16; count]
    }

    fn make_speech(count: usize, amplitude: i16) -> Vec<i16> {
        vec![amplitude; count]
    }

    #[test]
    fn test_silence_detector_creation() {
        let detector = SilenceDetectorStation::new();
        assert!(!detector.is_speech_active());
    }

    #[test]
    fn test_silence_detector_detects_speech_start() {
        let mut detector = SilenceDetectorStation::new();

        let frame = AudioFrame::new(0, make_silence(160));
        let control = detector.process(&frame);
        assert!(control.is_none());
        assert!(!detector.is_speech_active());

        let frame = AudioFrame::new(1, make_speech(160, 3000));
        let control = detector.process(&frame);
        assert_eq!(control, Some(ControlEvent::SpeechStart));
        assert!(detector.is_speech_active());
    }

    #[test]
    fn test_silence_detector_no_event_during_speech() {
        let mut detector = SilenceDetectorStation::new();

        let frame = AudioFrame::new(0, make_speech(160, 3000));
        detector.process(&frame);
        assert!(detector.is_speech_active());

        let frame = AudioFrame::new(1, make_speech(160, 3000));
        let control = detector.process(&frame);
        assert!(control.is_none());
    }

    #[test]
    fn test_silence_detector_reset() {
        let mut detector = SilenceDetectorStation::new();

        let frame = AudioFrame::new(0, make_speech(160, 3000));
        detector.process(&frame);
        assert!(detector.is_speech_active());

        detector.reset();
        assert!(!detector.is_speech_active());
    }

    #[test]
    fn test_auto_level_basic() {
        let mut al = AutoLevel::new();

        // Feed some silence levels
        for _ in 0..50 {
            al.update(0.005, false);
        }

        // Ambient should track the silence level
        assert!(al.ambient() < 0.01);

        // Threshold should be above ambient
        let threshold = al.update(0.005, false);
        assert!(threshold > al.ambient());
    }

    #[test]
    fn test_auto_level_ignores_speech() {
        let mut al = AutoLevel::new();

        // Establish baseline
        for _ in 0..50 {
            al.update(0.01, false);
        }
        let baseline = al.ambient();

        // Feed loud speech - should not affect ambient
        for _ in 0..20 {
            al.update(0.5, true);
        }

        // Ambient should be similar to baseline
        assert!((al.ambient() - baseline).abs() < 0.01);
    }

    #[test]
    fn test_silence_detector_with_auto_level() {
        let config = SilenceDetectorConfig {
            auto_level: true,
            show_levels: false,
            ..Default::default()
        };
        let mut detector = SilenceDetectorStation::with_config(config);

        // Process some frames
        for i in 0..10 {
            let frame = AudioFrame::new(i, make_silence(160));
            detector.process(&frame);
        }

        // Should have auto-level active
        assert!(detector.auto_level.is_some());
    }

    #[tokio::test]
    async fn test_silence_detector_run_forwards_audio() {
        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        let config = SilenceDetectorConfig {
            auto_level: false, // Disable for predictable test
            show_levels: false,
            ..Default::default()
        };
        let detector = SilenceDetectorStation::with_config(config);

        tokio::spawn(async move {
            detector.run(input_rx, output_tx).await;
        });

        input_tx
            .send(AudioFrame::new(0, make_speech(160, 3000)))
            .await
            .unwrap();

        let frame = output_rx.recv().await.unwrap();
        assert!(matches!(
            frame,
            PipelineFrame::Control(ControlEvent::SpeechStart)
        ));

        let frame = output_rx.recv().await.unwrap();
        assert!(frame.is_audio());

        drop(input_tx);
    }

    #[test]
    fn test_silence_without_speech_active() {
        let mut detector = SilenceDetectorStation::new();

        // Process silence frames without prior speech
        for i in 0..10 {
            let frame = AudioFrame::new(i, make_silence(160));
            let control = detector.process(&frame);
            assert!(
                control.is_none(),
                "Silence without speech should return None"
            );
        }

        assert!(!detector.is_speech_active());
    }

    #[test]
    fn test_speech_event_resets_flush_sent() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: false,
            flush_pause_ms: 100, // Shorter duration for testing
            sample_rate: 16000,
            vad: VadConfig {
                silence_duration_ms: 2000, // Long enough to not trigger SpeechEnd
                ..Default::default()
            },
        };
        let mut detector = SilenceDetectorStation::with_config(config);

        // Trigger speech start
        let frame = AudioFrame::new(0, make_speech(160, 3000));
        let control = detector.process(&frame);
        assert_eq!(control, Some(ControlEvent::SpeechStart));

        // Send silence frames and wait for flush_pause_ms to elapse
        for i in 1..10 {
            let frame = AudioFrame::new(i, make_silence(160));
            detector.process(&frame);
        }

        // Wait for flush pause duration to pass
        std::thread::sleep(std::time::Duration::from_millis(110));

        // Next silence frame should trigger flush
        let frame = AudioFrame::new(10, make_silence(160));
        let control = detector.process(&frame);
        assert_eq!(
            control,
            Some(ControlEvent::FlushChunk),
            "First flush should occur"
        );

        // Send speech again - this should reset flush_sent
        let frame = AudioFrame::new(11, make_speech(160, 3000));
        let control = detector.process(&frame);
        assert!(control.is_none(), "Speech event should return None");

        // Send silence again and wait
        for i in 12..20 {
            let frame = AudioFrame::new(i, make_silence(160));
            detector.process(&frame);
        }

        std::thread::sleep(std::time::Duration::from_millis(110));

        let frame = AudioFrame::new(20, make_silence(160));
        let control = detector.process(&frame);
        assert_eq!(
            control,
            Some(ControlEvent::FlushChunk),
            "Second flush should occur after speech reset flush_sent"
        );
    }

    #[test]
    fn test_flush_sent_only_once_per_pause() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: false,
            flush_pause_ms: 100, // Shorter duration for testing
            sample_rate: 16000,
            vad: VadConfig {
                silence_duration_ms: 2000, // Long enough to not trigger SpeechEnd
                ..Default::default()
            },
        };
        let mut detector = SilenceDetectorStation::with_config(config);

        // Trigger speech start
        let frame = AudioFrame::new(0, make_speech(160, 3000));
        detector.process(&frame);

        // Send silence frames
        for i in 1..10 {
            let frame = AudioFrame::new(i, make_silence(160));
            detector.process(&frame);
        }

        // Wait for flush pause duration
        std::thread::sleep(std::time::Duration::from_millis(110));

        let frame = AudioFrame::new(10, make_silence(160));
        let control = detector.process(&frame);
        assert_eq!(
            control,
            Some(ControlEvent::FlushChunk),
            "First flush should occur"
        );

        // More silence should return None (flush already sent)
        for i in 11..20 {
            let frame = AudioFrame::new(i, make_silence(160));
            let control = detector.process(&frame);
            assert!(
                control.is_none(),
                "Subsequent silence should not trigger flush"
            );
        }
    }

    #[test]
    fn test_speech_end_event() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: false,
            vad: VadConfig {
                silence_duration_ms: 100, // Short duration for testing
                ..Default::default()
            },
            flush_pause_ms: 50, // Shorter than silence_duration_ms
            ..Default::default()
        };
        let mut detector = SilenceDetectorStation::with_config(config);

        // Trigger speech start
        let frame = AudioFrame::new(0, make_speech(160, 3000));
        let control = detector.process(&frame);
        assert_eq!(control, Some(ControlEvent::SpeechStart));
        assert!(detector.is_speech_active());

        // Send silence frames
        for i in 1..10 {
            let frame = AudioFrame::new(i, make_silence(160));
            detector.process(&frame);
        }

        // Wait for silence_duration_ms to pass
        std::thread::sleep(std::time::Duration::from_millis(110));

        // Next silence frame should trigger SpeechEnd
        let frame = AudioFrame::new(10, make_silence(160));
        let control = detector.process(&frame);
        assert_eq!(
            control,
            Some(ControlEvent::SpeechEnd),
            "SpeechEnd event should be emitted"
        );
        assert!(
            !detector.is_speech_active(),
            "speech_active should be false"
        );
    }

    #[test]
    fn test_auto_level_skips_first_10_samples() {
        let mut al = AutoLevel::new();
        let initial_ambient = al.ambient();

        // Feed 10 samples with a specific level
        for _ in 0..10 {
            al.update(0.05, false);
        }

        // Ambient should still be at initial value (0.01)
        assert_eq!(
            al.ambient(),
            initial_ambient,
            "First 10 samples should not update ambient"
        );

        // 11th sample should update ambient
        al.update(0.05, false);
        assert!(
            al.ambient() != initial_ambient,
            "11th sample should update ambient"
        );
    }

    #[test]
    fn test_auto_level_fast_vs_slow_learning() {
        // Fast learning (first 100 samples)
        let mut al_fast = AutoLevel::new();
        // Skip first 10
        for _ in 0..11 {
            al_fast.update(0.0, false);
        }
        // Feed target level
        for _ in 0..50 {
            al_fast.update(0.05, false);
        }
        let fast_ambient = al_fast.ambient();

        // Slow learning (after 100 samples)
        let mut al_slow = AutoLevel::new();
        // Skip first 10
        for _ in 0..11 {
            al_slow.update(0.0, false);
        }
        // Feed 90 more to cross 100 threshold
        for _ in 0..90 {
            al_slow.update(0.0, false);
        }
        // Now feed target level for same number of iterations
        for _ in 0..50 {
            al_slow.update(0.05, false);
        }
        let slow_ambient = al_slow.ambient();

        // Fast learning should converge more than slow learning
        assert!(
            fast_ambient > slow_ambient,
            "Fast learning (alpha=0.1) should converge faster than slow learning (alpha=0.05)"
        );
    }

    #[test]
    fn test_auto_level_threshold_floor() {
        let mut al = AutoLevel::new();

        // Feed very low levels
        for _ in 0..50 {
            let threshold = al.update(0.0001, false);
            assert!(
                threshold >= 0.01,
                "Threshold should never go below min_threshold (0.01)"
            );
        }
    }

    #[test]
    fn test_display_level_doesnt_panic() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: true, // Enable level display
            ..Default::default()
        };
        let mut detector = SilenceDetectorStation::with_config(config);

        // Process a speech frame which triggers display_level
        let frame = AudioFrame::new(0, make_speech(160, 3000));
        detector.process(&frame);

        // If we got here without panic, test passes
    }

    #[test]
    fn test_clear_level_display_with_levels_enabled() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: true,
            ..Default::default()
        };
        let detector = SilenceDetectorStation::with_config(config);

        // Call clear_level_display - should write to stderr but not panic
        detector.clear_level_display();
    }

    #[test]
    fn test_clear_level_display_with_levels_disabled() {
        let config = SilenceDetectorConfig {
            auto_level: false,
            show_levels: false,
            ..Default::default()
        };
        let detector = SilenceDetectorStation::with_config(config);

        // Call clear_level_display - should be a no-op
        detector.clear_level_display();
    }
}
