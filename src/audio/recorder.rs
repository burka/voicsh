use crate::defaults;
use crate::error::{Result, VoicshError};

/// Trait for audio source devices.
///
/// This trait allows swapping implementations (real audio device vs mock).
pub trait AudioSource: Send + Sync {
    /// Start capturing audio from the source.
    ///
    /// # Returns
    /// Ok(()) if the source started successfully, or an error
    fn start(&mut self) -> Result<()>;

    /// Stop capturing audio from the source.
    ///
    /// # Returns
    /// Ok(()) if the source stopped successfully, or an error
    fn stop(&mut self) -> Result<()>;

    /// Read audio samples from the source.
    ///
    /// # Returns
    /// Vector of 16-bit PCM audio samples, or an error
    fn read_samples(&mut self) -> Result<Vec<i16>>;

    /// Returns true if this source will eventually stop producing samples (file, pipe).
    /// Returns false for live sources (microphone) where empty reads are normal.
    /// The audio polling loop uses this to distinguish "no data yet" from "source exhausted".
    fn is_finite(&self) -> bool {
        false
    }
}

/// Blanket implementation for `Box<dyn AudioSource>` to enable trait object usage
/// in generic contexts like `RingBuffer<A: AudioSource>`.
impl AudioSource for Box<dyn AudioSource> {
    fn start(&mut self) -> Result<()> {
        (**self).start()
    }

    fn stop(&mut self) -> Result<()> {
        (**self).stop()
    }

    fn read_samples(&mut self) -> Result<Vec<i16>> {
        (**self).read_samples()
    }

    fn is_finite(&self) -> bool {
        (**self).is_finite()
    }
}

/// Configuration for audio source initialization
#[derive(Debug, Clone)]
pub struct AudioSourceConfig {
    pub sample_rate: u32,
}

impl Default for AudioSourceConfig {
    fn default() -> Self {
        Self {
            sample_rate: defaults::SAMPLE_RATE,
        }
    }
}

/// A phase in a frame sequence: specific samples repeated `count` times.
#[derive(Debug, Clone)]
pub struct FramePhase {
    /// Samples to return for each read in this phase.
    pub samples: Vec<i16>,
    /// Number of reads to serve this phase.
    pub count: u32,
}

/// Mock audio source for testing
#[derive(Debug, Clone)]
pub struct MockAudioSource {
    is_started: bool,
    force_live: bool,
    samples: Vec<i16>,
    should_fail_start: bool,
    should_fail_stop: bool,
    should_fail_read: bool,
    error_message: String,
    frame_sequence: Option<Vec<FramePhase>>,
    sequence_index: usize,
    sequence_remaining: u32,
}

impl MockAudioSource {
    /// Create a new mock audio source with default settings
    pub fn new() -> Self {
        Self {
            is_started: false,
            force_live: false,
            samples: vec![0i16; 160],
            should_fail_start: false,
            should_fail_stop: false,
            should_fail_read: false,
            error_message: "mock audio error".to_string(),
            frame_sequence: None,
            sequence_index: 0,
            sequence_remaining: 0,
        }
    }

    /// Configure the mock to return specific samples
    pub fn with_samples(mut self, samples: Vec<i16>) -> Self {
        self.samples = samples;
        self
    }

    /// Configure the mock with a sequence of frame phases.
    ///
    /// Each phase defines samples and a repeat count. After all phases
    /// are exhausted, `read_samples` returns empty (signaling end).
    pub fn with_frame_sequence(mut self, phases: Vec<FramePhase>) -> Self {
        if let Some(first) = phases.first() {
            self.sequence_remaining = first.count;
        }
        self.frame_sequence = Some(phases);
        self.sequence_index = 0;
        self
    }

    /// Configure the mock to fail on start
    pub fn with_start_failure(mut self) -> Self {
        self.should_fail_start = true;
        self
    }

    /// Configure the mock to fail on stop
    pub fn with_stop_failure(mut self) -> Self {
        self.should_fail_stop = true;
        self
    }

    /// Configure the mock to fail on read
    pub fn with_read_failure(mut self) -> Self {
        self.should_fail_read = true;
        self
    }

    /// Configure the error message for failures
    pub fn with_error_message(mut self, message: &str) -> Self {
        self.error_message = message.to_string();
        self
    }

    /// Mark this source as a live (infinite) source.
    ///
    /// Live sources return `is_finite() == false`, meaning empty reads
    /// are not treated as end-of-stream by the pipeline. Overrides the
    /// default inference from `frame_sequence`.
    pub fn as_live_source(mut self) -> Self {
        self.force_live = true;
        self
    }

    /// Check if the audio source is started
    pub fn is_started(&self) -> bool {
        self.is_started
    }
}

impl Default for MockAudioSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSource for MockAudioSource {
    fn start(&mut self) -> Result<()> {
        if self.should_fail_start {
            Err(VoicshError::AudioCapture {
                message: self.error_message.clone(),
            })
        } else {
            self.is_started = true;
            Ok(())
        }
    }

    fn stop(&mut self) -> Result<()> {
        if self.should_fail_stop {
            Err(VoicshError::AudioCapture {
                message: self.error_message.clone(),
            })
        } else {
            self.is_started = false;
            Ok(())
        }
    }

    fn is_finite(&self) -> bool {
        self.frame_sequence.is_some() && !self.force_live
    }

    fn read_samples(&mut self) -> Result<Vec<i16>> {
        if self.should_fail_read {
            return Err(VoicshError::AudioCapture {
                message: self.error_message.clone(),
            });
        }

        // If a frame sequence is configured, walk through it
        if let Some(ref phases) = self.frame_sequence {
            if self.sequence_index >= phases.len() {
                return Ok(Vec::new()); // Exhausted
            }

            let samples = phases[self.sequence_index].samples.clone();
            self.sequence_remaining -= 1;

            if self.sequence_remaining == 0 {
                self.sequence_index += 1;
                if self.sequence_index < phases.len() {
                    self.sequence_remaining = phases[self.sequence_index].count;
                }
            }

            return Ok(samples);
        }

        Ok(self.samples.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_audio_source_returns_configured_samples() {
        let test_samples = vec![100i16, 200, 300, 400, 500];
        let mut source = MockAudioSource::new().with_samples(test_samples.clone());

        let result = source.read_samples();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), test_samples);
    }

    #[test]
    fn test_mock_audio_source_returns_default_samples() {
        let mut source = MockAudioSource::new();

        let result = source.read_samples();

        assert!(result.is_ok());
        let samples = result.unwrap();
        assert_eq!(samples.len(), 160);
        assert!(samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn test_mock_audio_source_returns_read_error_when_configured() {
        let mut source = MockAudioSource::new().with_read_failure();

        let result = source.read_samples();

        assert!(result.is_err());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "mock audio error");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_mock_audio_source_returns_custom_read_error() {
        let mut source = MockAudioSource::new()
            .with_read_failure()
            .with_error_message("buffer overflow");

        let result = source.read_samples();

        assert!(result.is_err());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "buffer overflow");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_mock_audio_source_start_stop_state_management() {
        let mut source = MockAudioSource::new();

        // Initially not started
        assert!(!source.is_started());

        // Start the source
        let start_result = source.start();
        assert!(start_result.is_ok());
        assert!(source.is_started());

        // Stop the source
        let stop_result = source.stop();
        assert!(stop_result.is_ok());
        assert!(!source.is_started());
    }

    #[test]
    fn test_mock_audio_source_start_failure() {
        let mut source = MockAudioSource::new().with_start_failure();

        let result = source.start();

        assert!(result.is_err());
        assert!(!source.is_started());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "mock audio error");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_mock_audio_source_stop_failure() {
        let mut source = MockAudioSource::new().with_stop_failure();

        // Start first
        source.start().unwrap();
        assert!(source.is_started());

        // Try to stop
        let result = source.stop();

        assert!(result.is_err());
        // State should remain started since stop failed
        assert!(source.is_started());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "mock audio error");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_mock_audio_source_custom_start_error() {
        let mut source = MockAudioSource::new()
            .with_start_failure()
            .with_error_message("device not found");

        let result = source.start();

        assert!(result.is_err());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "device not found");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_mock_audio_source_is_not_finite_by_default() {
        let source = MockAudioSource::new();
        assert!(
            !source.is_finite(),
            "Mock without frame sequence should be live (not finite)"
        );
    }

    #[test]
    fn test_mock_audio_source_is_finite_with_frame_sequence() {
        let source = MockAudioSource::new().with_frame_sequence(vec![FramePhase {
            samples: vec![0i16; 160],
            count: 5,
        }]);
        assert!(
            source.is_finite(),
            "Mock with frame sequence should be finite"
        );
    }

    #[test]
    fn test_is_finite_through_trait_object() {
        let live: Box<dyn AudioSource> = Box::new(MockAudioSource::new());
        assert!(
            !live.is_finite(),
            "Live source through trait object should be not finite"
        );

        let finite: Box<dyn AudioSource> =
            Box::new(MockAudioSource::new().with_frame_sequence(vec![FramePhase {
                samples: vec![0i16; 160],
                count: 1,
            }]));
        assert!(
            finite.is_finite(),
            "Finite source through trait object should be finite"
        );
    }

    #[test]
    fn test_audio_source_config_default() {
        let config = AudioSourceConfig::default();
        assert_eq!(config.sample_rate, 16000);
    }

    #[test]
    fn test_audio_source_config_custom() {
        let config = AudioSourceConfig { sample_rate: 44100 };
        assert_eq!(config.sample_rate, 44100);
    }

    #[test]
    fn test_audio_source_trait_is_object_safe() {
        // Verify that we can use Box<dyn AudioSource>
        let source: Box<dyn AudioSource> =
            Box::new(MockAudioSource::new().with_samples(vec![1i16, 2, 3, 4, 5]));

        let mut boxed_source = source;
        let start_result = boxed_source.start();
        assert!(start_result.is_ok());

        let samples_result = boxed_source.read_samples();
        assert!(samples_result.is_ok());
        assert_eq!(samples_result.unwrap(), vec![1i16, 2, 3, 4, 5]);

        let stop_result = boxed_source.stop();
        assert!(stop_result.is_ok());
    }

    #[test]
    fn test_mock_audio_source_builder_pattern() {
        // Test that builder pattern methods can be chained
        let mut source = MockAudioSource::new()
            .with_samples(vec![10i16, 20, 30])
            .with_error_message("custom error")
            .with_samples(vec![40i16, 50, 60]);

        let result = source.read_samples().unwrap();
        assert_eq!(result, vec![40i16, 50, 60]);
    }

    #[test]
    fn test_mock_audio_source_multiple_reads() {
        let test_samples = vec![1i16, 2, 3];
        let mut source = MockAudioSource::new().with_samples(test_samples.clone());

        // Multiple reads should return the same samples
        let result1 = source.read_samples();
        let result2 = source.read_samples();
        let result3 = source.read_samples();

        assert_eq!(result1.unwrap(), test_samples);
        assert_eq!(result2.unwrap(), test_samples);
        assert_eq!(result3.unwrap(), test_samples);
    }

    #[test]
    fn test_mock_audio_source_start_stop_multiple_times() {
        let mut source = MockAudioSource::new();

        // Start and stop multiple times
        for _ in 0..3 {
            assert!(source.start().is_ok());
            assert!(source.is_started());
            assert!(source.stop().is_ok());
            assert!(!source.is_started());
        }
    }

    #[test]
    fn test_mock_audio_source_read_while_stopped() {
        let mut source = MockAudioSource::new().with_samples(vec![10i16, 20, 30]);

        // Should be able to read even when not started
        let result = source.read_samples();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![10i16, 20, 30]);
    }

    #[test]
    fn test_mock_audio_source_empty_samples() {
        let mut source = MockAudioSource::new().with_samples(vec![]);

        let result = source.read_samples();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Vec::<i16>::new());
    }

    #[test]
    fn test_mock_audio_source_large_samples() {
        // Simulate 1 second of 16kHz audio
        let large_samples = vec![0i16; 16000];
        let mut source = MockAudioSource::new().with_samples(large_samples.clone());

        let result = source.read_samples();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 16000);
    }

    #[test]
    fn test_mock_audio_source_default_trait() {
        let source = MockAudioSource::default();
        assert!(!source.is_started());
    }

    #[test]
    fn test_audio_source_config_clone() {
        let config1 = AudioSourceConfig { sample_rate: 48000 };
        let config2 = config1.clone();
        assert_eq!(config1.sample_rate, config2.sample_rate);
    }

    #[test]
    fn test_mock_audio_source_not_finite_by_default() {
        // Plain MockAudioSource has no frame_sequence, so it's live (infinite)
        let source = MockAudioSource::new();
        assert!(
            !source.is_finite(),
            "MockAudioSource without frame_sequence should be live (not finite)"
        );
    }

    #[test]
    fn test_mock_audio_source_as_live_source() {
        let source = MockAudioSource::new().as_live_source();
        assert!(
            !source.is_finite(),
            "as_live_source() should make the source infinite"
        );
    }

    #[test]
    fn test_mock_audio_source_clone() {
        let source1 = MockAudioSource::new().with_samples(vec![1i16, 2, 3]);
        let mut source2 = source1.clone();

        let result = source2.read_samples();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1i16, 2, 3]);
    }

    // Resource exhaustion tests (simulated)
    #[test]
    fn test_mock_audio_source_extremely_large_buffer() {
        // Simulate 10 minutes of audio at 16kHz (9.6 million samples)
        const TEN_MINUTES_AT_16KHZ: usize = 10 * 60 * 16000;
        let large_samples = vec![100i16; TEN_MINUTES_AT_16KHZ];
        let mut source = MockAudioSource::new().with_samples(large_samples.clone());

        let result = source.read_samples();
        assert!(result.is_ok(), "Should handle large buffers");
        let returned_samples = result.unwrap();
        assert_eq!(
            returned_samples.len(),
            TEN_MINUTES_AT_16KHZ,
            "Should return all samples"
        );
        assert_eq!(returned_samples[0], 100, "First sample should be correct");
        assert_eq!(
            returned_samples[TEN_MINUTES_AT_16KHZ - 1],
            100,
            "Last sample should be correct"
        );
    }

    #[test]
    fn test_mock_audio_source_rapid_read_cycles() {
        // Simulate 1000 rapid read cycles
        let samples = vec![42i16; 16000]; // 1 second of audio
        let mut source = MockAudioSource::new().with_samples(samples.clone());

        // Perform 1000 reads rapidly
        for i in 0..1000 {
            let result = source.read_samples();
            assert!(result.is_ok(), "Read {} should succeed", i);
            let data = result.unwrap();
            assert_eq!(data.len(), 16000, "Read {} should return correct length", i);
        }
    }

    #[test]
    fn test_mock_audio_source_maximum_sample_values() {
        // Test with extreme sample values (min/max i16)
        let extreme_samples = vec![
            i16::MIN,
            i16::MIN + 1,
            -1000,
            0,
            1000,
            i16::MAX - 1,
            i16::MAX,
        ];
        let mut source = MockAudioSource::new().with_samples(extreme_samples.clone());

        let result = source.read_samples();
        assert!(result.is_ok(), "Should handle extreme sample values");
        let returned = result.unwrap();
        assert_eq!(
            returned, extreme_samples,
            "Extreme values should be preserved"
        );
    }

    #[test]
    fn test_mock_audio_source_empty_then_large() {
        // Start with empty, then switch to large buffer
        let mut source = MockAudioSource::new().with_samples(vec![]);

        let result1 = source.read_samples();
        assert!(result1.is_ok(), "Empty read should succeed");
        assert_eq!(result1.unwrap().len(), 0, "Should return empty");

        // Now configure with large buffer
        let large_buffer = vec![99i16; 1_000_000]; // 1 million samples
        source = source.with_samples(large_buffer.clone());

        let result2 = source.read_samples();
        assert!(result2.is_ok(), "Large read should succeed");
        assert_eq!(
            result2.unwrap().len(),
            1_000_000,
            "Should return all million samples"
        );
    }

    #[test]
    fn test_mock_audio_source_start_stop_stress() {
        // Stress test start/stop cycles
        let mut source = MockAudioSource::new();

        for i in 0..100 {
            let start_result = source.start();
            assert!(start_result.is_ok(), "Start {} should succeed", i);
            assert!(
                source.is_started(),
                "Should be started after iteration {}",
                i
            );

            let stop_result = source.stop();
            assert!(stop_result.is_ok(), "Stop {} should succeed", i);
            assert!(
                !source.is_started(),
                "Should be stopped after iteration {}",
                i
            );
        }
    }

    #[test]
    fn test_mock_audio_source_read_during_rapid_start_stop() {
        // Read while rapidly starting and stopping
        let samples = vec![77i16; 1000];
        let mut source = MockAudioSource::new().with_samples(samples.clone());

        for i in 0..50 {
            source.start().unwrap();
            let result = source.read_samples();
            assert!(result.is_ok(), "Read during cycle {} should succeed", i);
            source.stop().unwrap();

            // Also read while stopped
            let result2 = source.read_samples();
            assert!(
                result2.is_ok(),
                "Read while stopped in cycle {} should succeed",
                i
            );
        }
    }
}
