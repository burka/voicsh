use crate::defaults;
use crate::error::{Result, VoicshError};
use std::path::PathBuf;
use std::sync::Arc;

/// Trait for speech-to-text transcription.
///
/// This trait allows swapping implementations (real Whisper vs mock).
pub trait Transcriber: Send + Sync {
    /// Transcribe audio samples to text.
    ///
    /// # Arguments
    /// * `audio` - Audio samples as 16-bit PCM at 16kHz mono
    ///
    /// # Returns
    /// Transcribed text or error
    fn transcribe(&self, audio: &[i16]) -> Result<String>;

    /// Get the name of the loaded model
    fn model_name(&self) -> &str;

    /// Check if the transcriber is ready
    fn is_ready(&self) -> bool;
}

/// Implement Transcriber for Arc<T> to allow sharing across sessions.
impl<T: Transcriber> Transcriber for Arc<T> {
    fn transcribe(&self, audio: &[i16]) -> Result<String> {
        (**self).transcribe(audio)
    }

    fn model_name(&self) -> &str {
        (**self).model_name()
    }

    fn is_ready(&self) -> bool {
        (**self).is_ready()
    }
}

/// Configuration for transcriber initialization
#[derive(Debug, Clone)]
pub struct TranscriberConfig {
    pub model_path: PathBuf,
    pub language: String,
}

impl Default for TranscriberConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from(""),
            language: defaults::DEFAULT_LANGUAGE.to_string(),
        }
    }
}

/// Mock transcriber for testing
#[derive(Debug, Clone)]
pub struct MockTranscriber {
    model_name: String,
    response: String,
    should_fail: bool,
}

impl MockTranscriber {
    /// Create a new mock transcriber with default settings
    pub fn new(model_name: &str) -> Self {
        Self {
            model_name: model_name.to_string(),
            response: "mock transcription".to_string(),
            should_fail: false,
        }
    }

    /// Configure the mock to return a specific response
    pub fn with_response(mut self, response: &str) -> Self {
        self.response = response.to_string();
        self
    }

    /// Configure the mock to fail on transcribe
    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, _audio: &[i16]) -> Result<String> {
        if self.should_fail {
            Err(VoicshError::Transcription {
                message: "mock transcription failure".to_string(),
            })
        } else {
            Ok(self.response.clone())
        }
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn is_ready(&self) -> bool {
        !self.should_fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_transcriber_returns_response() {
        let transcriber = MockTranscriber::new("test-model").with_response("Hello, this is a test");

        let audio = vec![0i16; 1000];
        let result = transcriber.transcribe(&audio);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, this is a test");
    }

    #[test]
    fn test_mock_transcriber_returns_error_when_configured() {
        let transcriber = MockTranscriber::new("test-model").with_failure();

        let audio = vec![0i16; 1000];
        let result = transcriber.transcribe(&audio);

        assert!(result.is_err());
        match result {
            Err(VoicshError::Transcription { message }) => {
                assert_eq!(message, "mock transcription failure");
            }
            _ => panic!("Expected Transcription error"),
        }
    }

    #[test]
    fn test_mock_transcriber_model_name() {
        let transcriber = MockTranscriber::new("whisper-base");
        assert_eq!(transcriber.model_name(), "whisper-base");
    }

    #[test]
    fn test_mock_transcriber_is_ready() {
        let ready_transcriber = MockTranscriber::new("test-model");
        assert!(ready_transcriber.is_ready());

        let failing_transcriber = MockTranscriber::new("test-model").with_failure();
        assert!(!failing_transcriber.is_ready());
    }

    #[test]
    fn test_transcriber_trait_is_object_safe() {
        // Verify that we can use Box<dyn Transcriber>
        let transcriber: Box<dyn Transcriber> =
            Box::new(MockTranscriber::new("test-model").with_response("boxed test"));

        assert_eq!(transcriber.model_name(), "test-model");
        assert!(transcriber.is_ready());

        let audio = vec![0i16; 100];
        let result = transcriber.transcribe(&audio);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "boxed test");
    }

    #[test]
    fn test_transcriber_config_default() {
        let config = TranscriberConfig::default();
        assert_eq!(config.model_path, PathBuf::from(""));
        assert_eq!(config.language, "en");
    }

    #[test]
    fn test_transcriber_config_custom() {
        let config = TranscriberConfig {
            model_path: PathBuf::from("/path/to/model.bin"),
            language: "es".to_string(),
        };
        assert_eq!(config.model_path, PathBuf::from("/path/to/model.bin"));
        assert_eq!(config.language, "es");
    }

    #[test]
    fn test_mock_transcriber_builder_pattern() {
        // Test that builder pattern methods can be chained
        let transcriber = MockTranscriber::new("model")
            .with_response("first response")
            .with_response("second response");

        let audio = vec![0i16; 10];
        let result = transcriber.transcribe(&audio).unwrap();
        assert_eq!(result, "second response");
    }

    #[test]
    fn test_mock_transcriber_empty_audio() {
        let transcriber = MockTranscriber::new("test-model");
        let empty_audio: Vec<i16> = vec![];
        let result = transcriber.transcribe(&empty_audio);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_transcriber_large_audio() {
        let transcriber =
            MockTranscriber::new("test-model").with_response("long audio transcription");

        // Simulate 10 seconds of 16kHz audio
        let audio = vec![0i16; 16000 * 10];
        let result = transcriber.transcribe(&audio);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "long audio transcription");
    }
}
