//! Voice Activity Detection (VAD) module.
//!
//! Detects speech activity in audio streams using RMS-based thresholding
//! and state machine logic to handle silence intervals.

use crate::defaults;
use std::time::Instant;

/// Trait for time operations, allowing mock time in tests.
pub trait Clock: Send + Sync {
    /// Returns the current instant.
    fn now(&self) -> Instant;
}

/// Real system clock using `std::time::Instant::now()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Configuration for Voice Activity Detection.
#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// RMS threshold for detecting speech (0.0 to 1.0).
    pub speech_threshold: f32,
    /// Duration of silence before speech is considered ended (milliseconds).
    pub silence_duration_ms: u32,
    /// Minimum duration of speech before it's considered valid (milliseconds).
    pub min_speech_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            speech_threshold: defaults::VAD_THRESHOLD,
            silence_duration_ms: defaults::SILENCE_DURATION_MS,
            min_speech_ms: 300,
        }
    }
}

/// Current state of voice activity detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    /// No speech detected.
    Idle,
    /// Speech is being detected.
    Speaking,
    /// Silence detected, waiting to confirm speech end.
    MaybeSilence,
    /// Speech has ended.
    Stopped,
}

/// Events emitted by the VAD processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    /// Speech has started.
    SpeechStart,
    /// Ongoing speech detected.
    Speech,
    /// Silence detected during speech.
    Silence,
    /// Speech has ended.
    SpeechEnd,
}

/// Detailed VAD processing result with level information.
#[derive(Debug, Clone, Copy)]
pub struct VadResult {
    /// The VAD event
    pub event: VadEvent,
    /// Current RMS level (0.0 to 1.0)
    pub level: f32,
    /// Speech detection threshold
    pub threshold: f32,
    /// Milliseconds of silence (when in MaybeSilence state)
    pub silence_ms: u32,
    /// Silence duration needed to end speech
    pub silence_duration_ms: u32,
}

/// Voice Activity Detector state machine.
pub struct Vad<C: Clock = SystemClock> {
    config: VadConfig,
    state: VadState,
    silence_start: Option<Instant>,
    speech_start: Option<Instant>,
    clock: C,
}

impl<C: Clock> Vad<C> {
    /// Creates a new VAD instance with the given configuration and clock.
    pub fn with_clock(config: VadConfig, clock: C) -> Self {
        Self {
            config,
            state: VadState::Idle,
            silence_start: None,
            speech_start: None,
            clock,
        }
    }

    /// Processes audio samples and returns the corresponding VAD event.
    ///
    /// # Arguments
    /// * `samples` - Audio samples as 16-bit PCM
    /// * `sample_rate` - Sample rate in Hz (used for timing calculations)
    pub fn process(&mut self, samples: &[i16], sample_rate: u32) -> VadEvent {
        self.process_with_info(samples, sample_rate).event
    }

    /// Processes audio samples and returns detailed VAD result with level info.
    ///
    /// # Arguments
    /// * `samples` - Audio samples as 16-bit PCM
    /// * `sample_rate` - Sample rate in Hz (used for timing calculations)
    pub fn process_with_info(&mut self, samples: &[i16], _sample_rate: u32) -> VadResult {
        let rms = calculate_rms(samples);
        let is_speech = rms > self.config.speech_threshold;
        let now = self.clock.now();

        let (event, silence_ms) = match self.state {
            VadState::Idle => {
                if is_speech {
                    self.state = VadState::Speaking;
                    self.speech_start = Some(now);
                    self.silence_start = None;
                    (VadEvent::SpeechStart, 0)
                } else {
                    (VadEvent::Silence, 0)
                }
            }
            VadState::Speaking => {
                if is_speech {
                    self.silence_start = None;
                    (VadEvent::Speech, 0)
                } else {
                    self.state = VadState::MaybeSilence;
                    self.silence_start = Some(now);
                    (VadEvent::Silence, 0)
                }
            }
            VadState::MaybeSilence => {
                if is_speech {
                    self.state = VadState::Speaking;
                    self.silence_start = None;
                    (VadEvent::Speech, 0)
                } else {
                    let silence_elapsed = self
                        .silence_start
                        .map(|start| now.duration_since(start).as_millis() as u32)
                        .unwrap_or(0);

                    if silence_elapsed >= self.config.silence_duration_ms {
                        self.state = VadState::Stopped;
                        self.silence_start = None;
                        self.speech_start = None;
                        (VadEvent::SpeechEnd, silence_elapsed)
                    } else {
                        (VadEvent::Silence, silence_elapsed)
                    }
                }
            }
            VadState::Stopped => (VadEvent::Silence, 0),
        };

        VadResult {
            event,
            level: rms,
            threshold: self.config.speech_threshold,
            silence_ms,
            silence_duration_ms: self.config.silence_duration_ms,
        }
    }

    /// Returns the current VAD state.
    pub fn state(&self) -> VadState {
        self.state
    }

    /// Resets the VAD to idle state.
    pub fn reset(&mut self) {
        self.state = VadState::Idle;
        self.silence_start = None;
        self.speech_start = None;
    }

    /// Updates the speech threshold without resetting state.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.config.speech_threshold = threshold;
    }
}

impl Vad<SystemClock> {
    /// Creates a new VAD instance with the given configuration using the system clock.
    pub fn new(config: VadConfig) -> Self {
        Self::with_clock(config, SystemClock)
    }
}

/// Calculates the Root Mean Square (RMS) of audio samples.
///
/// # Arguments
/// * `samples` - Audio samples as 16-bit PCM
///
/// # Returns
/// Normalized RMS value (0.0 to 1.0), where:
/// - 0.0 represents silence
/// - ~0.707 represents a full-scale sine wave
/// - 1.0 represents maximum amplitude
pub fn calculate_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum_squares: f64 = samples
        .iter()
        .map(|&sample| {
            let normalized = sample as f64 / i16::MAX as f64;
            normalized * normalized
        })
        .sum();

    let mean_square = sum_squares / samples.len() as f64;
    mean_square.sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Mock clock for testing that allows manual time advancement.
    #[derive(Debug, Clone)]
    pub struct MockClock {
        current: Arc<Mutex<Instant>>,
    }

    impl MockClock {
        /// Creates a new mock clock starting at the current instant.
        pub fn new() -> Self {
            Self {
                current: Arc::new(Mutex::new(Instant::now())),
            }
        }

        /// Advances the mock clock by the given duration.
        pub fn advance(&self, duration: Duration) {
            let mut current = self.current.lock().unwrap();
            *current += duration;
        }
    }

    impl Clock for MockClock {
        fn now(&self) -> Instant {
            *self.current.lock().unwrap()
        }
    }

    fn make_silence(count: usize) -> Vec<i16> {
        vec![0i16; count]
    }

    fn make_speech(count: usize, amplitude: i16) -> Vec<i16> {
        vec![amplitude; count]
    }

    #[test]
    fn test_rms_silence_is_zero() {
        let silence = make_silence(1000);
        let rms = calculate_rms(&silence);
        assert_eq!(rms, 0.0);
    }

    #[test]
    fn test_rms_max_amplitude() {
        let max_signal = make_speech(1000, i16::MAX);
        let rms = calculate_rms(&max_signal);
        assert!((rms - 1.0).abs() < 0.001, "RMS should be ~1.0, got {}", rms);
    }

    #[test]
    fn test_rms_negative_samples() {
        let negative_signal = make_speech(1000, i16::MIN);
        let rms = calculate_rms(&negative_signal);
        // Negative samples should produce the same RMS as positive (squared)
        assert!(rms > 0.99, "RMS should be ~1.0 for i16::MIN, got {}", rms);
    }

    #[test]
    fn test_rms_mixed_positive_negative() {
        let mut mixed = make_speech(500, 1000);
        mixed.extend(make_speech(500, -1000));
        let rms = calculate_rms(&mixed);
        // RMS of ±1000 should be around 1000/32767 ≈ 0.0305
        assert!(
            rms > 0.025 && rms < 0.035,
            "RMS should be ~0.0305, got {}",
            rms
        );
    }

    #[test]
    fn test_vad_starts_idle() {
        let config = VadConfig::default();
        let vad = Vad::new(config);
        assert_eq!(vad.state(), VadState::Idle);
    }

    #[test]
    fn test_vad_detects_speech_start() {
        let config = VadConfig::default();
        let mut vad = Vad::new(config);

        // Process silence first
        let silence = make_silence(1000);
        let event = vad.process(&silence, 16000);
        assert_eq!(event, VadEvent::Silence);
        assert_eq!(vad.state(), VadState::Idle);

        // Process speech - should trigger SpeechStart
        let speech = make_speech(1000, 3000); // RMS ~0.09, above 0.02 threshold
        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::SpeechStart);
        assert_eq!(vad.state(), VadState::Speaking);
    }

    #[test]
    fn test_vad_stays_speaking_during_speech() {
        let config = VadConfig::default();
        let mut vad = Vad::new(config);

        let speech = make_speech(1000, 3000);

        // First speech triggers start
        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::SpeechStart);

        // Subsequent speech keeps speaking state
        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::Speech);
        assert_eq!(vad.state(), VadState::Speaking);

        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::Speech);
        assert_eq!(vad.state(), VadState::Speaking);
    }

    #[test]
    fn test_vad_detects_silence_after_speech() {
        let config = VadConfig::default();
        let mut vad = Vad::new(config);

        let speech = make_speech(1000, 3000);
        let silence = make_silence(1000);

        // Start speaking
        vad.process(&speech, 16000);
        assert_eq!(vad.state(), VadState::Speaking);

        // Process silence - should enter MaybeSilence
        let event = vad.process(&silence, 16000);
        assert_eq!(event, VadEvent::Silence);
        assert_eq!(vad.state(), VadState::MaybeSilence);
    }

    #[test]
    fn test_vad_returns_to_speaking_if_speech_resumes() {
        let config = VadConfig::default();
        let mut vad = Vad::new(config);

        let speech = make_speech(1000, 3000);
        let silence = make_silence(1000);

        // Start speaking
        vad.process(&speech, 16000);

        // Brief silence
        vad.process(&silence, 16000);
        assert_eq!(vad.state(), VadState::MaybeSilence);

        // Resume speaking - should go back to Speaking
        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::Speech);
        assert_eq!(vad.state(), VadState::Speaking);
    }

    #[test]
    fn test_vad_ends_speech_after_silence_duration() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 100, // Short duration for testing
            min_speech_ms: 50,
        };
        let clock = MockClock::new();
        let mut vad = Vad::with_clock(config, clock.clone());

        let speech = make_speech(1000, 3000);
        let silence = make_silence(1000);

        // Start speaking
        vad.process(&speech, 16000);
        assert_eq!(vad.state(), VadState::Speaking);

        // Process silence
        vad.process(&silence, 16000);
        assert_eq!(vad.state(), VadState::MaybeSilence);

        // Advance time to exceed silence threshold
        clock.advance(Duration::from_millis(150));

        // Process more silence - should trigger SpeechEnd
        let event = vad.process(&silence, 16000);
        assert_eq!(event, VadEvent::SpeechEnd);
        assert_eq!(vad.state(), VadState::Stopped);
    }

    #[test]
    fn test_vad_reset_returns_to_idle() {
        let config = VadConfig::default();
        let mut vad = Vad::new(config);

        let speech = make_speech(1000, 3000);

        // Start speaking
        vad.process(&speech, 16000);
        assert_eq!(vad.state(), VadState::Speaking);

        // Reset
        vad.reset();
        assert_eq!(vad.state(), VadState::Idle);

        // Process speech again - should trigger SpeechStart
        let event = vad.process(&speech, 16000);
        assert_eq!(event, VadEvent::SpeechStart);
    }

    #[test]
    fn test_vad_stopped_state_remains_silent() {
        let config = VadConfig {
            speech_threshold: 0.02,
            silence_duration_ms: 100,
            min_speech_ms: 50,
        };
        let clock = MockClock::new();
        let mut vad = Vad::with_clock(config, clock.clone());

        let speech = make_speech(1000, 3000);
        let silence = make_silence(1000);

        // Get to Stopped state
        vad.process(&speech, 16000);
        vad.process(&silence, 16000);
        clock.advance(Duration::from_millis(150));
        vad.process(&silence, 16000);
        assert_eq!(vad.state(), VadState::Stopped);

        // Further silence should keep returning Silence
        let event = vad.process(&silence, 16000);
        assert_eq!(event, VadEvent::Silence);
        assert_eq!(vad.state(), VadState::Stopped);
    }

    #[test]
    fn test_calculate_rms_empty_samples() {
        let empty: Vec<i16> = vec![];
        let rms = calculate_rms(&empty);
        assert_eq!(rms, 0.0);
    }
}
