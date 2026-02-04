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

/// Mock audio source for testing
#[derive(Debug, Clone)]
pub struct MockAudioSource {
    is_started: bool,
    samples: Vec<i16>,
    should_fail_start: bool,
    should_fail_stop: bool,
    should_fail_read: bool,
    error_message: String,
}

impl MockAudioSource {
    /// Create a new mock audio source with default settings
    pub fn new() -> Self {
        Self {
            is_started: false,
            samples: vec![0i16; 160],
            should_fail_start: false,
            should_fail_stop: false,
            should_fail_read: false,
            error_message: "mock audio error".to_string(),
        }
    }

    /// Configure the mock to return specific samples
    pub fn with_samples(mut self, samples: Vec<i16>) -> Self {
        self.samples = samples;
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

    fn read_samples(&mut self) -> Result<Vec<i16>> {
        if self.should_fail_read {
            Err(VoicshError::AudioCapture {
                message: self.error_message.clone(),
            })
        } else {
            Ok(self.samples.clone())
        }
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
    fn test_mock_audio_source_clone() {
        let source1 = MockAudioSource::new().with_samples(vec![1i16, 2, 3]);
        let mut source2 = source1.clone();

        let result = source2.read_samples();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1i16, 2, 3]);
    }
}
