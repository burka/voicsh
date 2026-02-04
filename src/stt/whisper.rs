//! Whisper-based speech-to-text transcription.
//!
//! This module provides a Whisper implementation of the Transcriber trait.
//! Currently, this is a placeholder implementation until the whisper-rs dependency is added.

use crate::defaults;
use crate::error::{Result, VoicshError};
use crate::stt::transcriber::Transcriber;
use std::path::PathBuf;

/// Configuration for Whisper transcriber.
#[derive(Debug, Clone)]
pub struct WhisperConfig {
    /// Path to the Whisper model file
    pub model_path: PathBuf,
    /// Language code (e.g., "en", "es", "fr")
    pub language: String,
    /// Number of threads for inference (None = auto-detect)
    pub threads: Option<usize>,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("models/ggml-base.bin"),
            language: defaults::DEFAULT_LANGUAGE.to_string(),
            threads: None,
        }
    }
}

/// Whisper-based transcriber implementation.
///
/// This is currently a placeholder implementation that validates configuration
/// but does not perform actual transcription. The real Whisper integration
/// will be added when the whisper-rs dependency is available.
#[derive(Debug)]
pub struct WhisperTranscriber {
    config: WhisperConfig,
    model_name: String,
    is_ready: bool,
}

impl WhisperTranscriber {
    /// Create a new Whisper transcriber.
    ///
    /// # Arguments
    /// * `config` - Configuration for the transcriber
    ///
    /// # Returns
    /// A new WhisperTranscriber instance or an error if the model file doesn't exist
    ///
    /// # Errors
    /// Returns `VoicshError::TranscriptionModelNotFound` if the model file doesn't exist
    pub fn new(config: WhisperConfig) -> Result<Self> {
        // Validate that the model file exists
        if !config.model_path.exists() {
            return Err(VoicshError::TranscriptionModelNotFound {
                path: config.model_path.to_string_lossy().to_string(),
            });
        }

        // Extract model name from the file path
        let model_name = config
            .model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self {
            config,
            model_name,
            is_ready: false, // Not ready until whisper-rs is integrated
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, _audio: &[i16]) -> Result<String> {
        // Placeholder implementation - returns error until whisper-rs is integrated
        Err(VoicshError::TranscriptionModelNotFound {
            path: self.config.model_path.to_string_lossy().to_string(),
        })
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn is_ready(&self) -> bool {
        self.is_ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_whisper_config_default() {
        let config = WhisperConfig::default();
        assert_eq!(config.model_path, PathBuf::from("models/ggml-base.bin"));
        assert_eq!(config.language, "en");
        assert_eq!(config.threads, None);
    }

    #[test]
    fn test_whisper_config_custom() {
        let config = WhisperConfig {
            model_path: PathBuf::from("/custom/model.bin"),
            language: "es".to_string(),
            threads: Some(4),
        };
        assert_eq!(config.model_path, PathBuf::from("/custom/model.bin"));
        assert_eq!(config.language, "es");
        assert_eq!(config.threads, Some(4));
    }

    #[test]
    fn test_whisper_transcriber_new_fails_for_missing_model() {
        let config = WhisperConfig {
            model_path: PathBuf::from("/nonexistent/model.bin"),
            language: "en".to_string(),
            threads: None,
        };

        let result = WhisperTranscriber::new(config);
        assert!(result.is_err());

        match result {
            Err(VoicshError::TranscriptionModelNotFound { path }) => {
                assert_eq!(path, "/nonexistent/model.bin");
            }
            _ => panic!("Expected TranscriptionModelNotFound error"),
        }
    }

    #[test]
    fn test_whisper_transcriber_new_succeeds_for_existing_model() {
        // Create a temporary file to simulate a model file
        let temp_file = NamedTempFile::new().unwrap();
        let model_path = temp_file.path().to_path_buf();

        let config = WhisperConfig {
            model_path: model_path.clone(),
            language: "en".to_string(),
            threads: Some(2),
        };

        let result = WhisperTranscriber::new(config);
        assert!(result.is_ok());

        let transcriber = result.unwrap();
        assert_eq!(transcriber.config().model_path, model_path);
        assert_eq!(transcriber.config().language, "en");
        assert_eq!(transcriber.config().threads, Some(2));
    }

    #[test]
    fn test_whisper_transcriber_model_name() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();

        // Create a path with a meaningful model name
        let model_dir = temp_path.parent().unwrap();
        let model_path = model_dir.join("ggml-base.bin");
        std::fs::write(&model_path, b"fake model data").unwrap();

        let config = WhisperConfig {
            model_path: model_path.clone(),
            language: "en".to_string(),
            threads: None,
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        assert_eq!(transcriber.model_name(), "ggml-base");

        // Cleanup
        std::fs::remove_file(&model_path).unwrap();
    }

    #[test]
    fn test_whisper_transcriber_is_ready() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = WhisperConfig {
            model_path: temp_file.path().to_path_buf(),
            language: "en".to_string(),
            threads: None,
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        // Not ready until whisper-rs is integrated
        assert!(!transcriber.is_ready());
    }

    #[test]
    fn test_whisper_transcriber_transcribe_returns_error() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = WhisperConfig {
            model_path: temp_file.path().to_path_buf(),
            language: "en".to_string(),
            threads: None,
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        let audio = vec![0i16; 1000];
        let result = transcriber.transcribe(&audio);

        assert!(result.is_err());
        match result {
            Err(VoicshError::TranscriptionModelNotFound { .. }) => {
                // Expected error
            }
            _ => panic!("Expected TranscriptionModelNotFound error"),
        }
    }

    #[test]
    fn test_whisper_transcriber_implements_transcriber_trait() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = WhisperConfig {
            model_path: temp_file.path().to_path_buf(),
            language: "en".to_string(),
            threads: None,
        };

        // Test that we can use Box<dyn Transcriber>
        let transcriber: Box<dyn Transcriber> = Box::new(WhisperTranscriber::new(config).unwrap());

        assert!(!transcriber.is_ready());
        assert!(!transcriber.model_name().is_empty());

        let audio = vec![0i16; 100];
        let result = transcriber.transcribe(&audio);
        assert!(result.is_err());
    }

    #[test]
    fn test_whisper_transcriber_empty_audio() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = WhisperConfig {
            model_path: temp_file.path().to_path_buf(),
            language: "en".to_string(),
            threads: None,
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        let empty_audio: Vec<i16> = vec![];
        let result = transcriber.transcribe(&empty_audio);
        assert!(result.is_err());
    }

    #[test]
    fn test_whisper_transcriber_large_audio() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = WhisperConfig {
            model_path: temp_file.path().to_path_buf(),
            language: "en".to_string(),
            threads: None,
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        // Simulate 10 seconds of 16kHz audio
        let audio = vec![0i16; 16000 * 10];
        let result = transcriber.transcribe(&audio);
        assert!(result.is_err());
    }

    #[test]
    fn test_whisper_config_clone() {
        let config = WhisperConfig::default();
        let cloned = config.clone();
        assert_eq!(config.model_path, cloned.model_path);
        assert_eq!(config.language, cloned.language);
        assert_eq!(config.threads, cloned.threads);
    }

    #[test]
    fn test_whisper_config_debug() {
        let config = WhisperConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("WhisperConfig"));
        assert!(debug_str.contains("model_path"));
        assert!(debug_str.contains("language"));
    }

    #[test]
    fn test_whisper_transcriber_config_accessor() {
        let temp_file = NamedTempFile::new().unwrap();
        let model_path = temp_file.path().to_path_buf();

        let config = WhisperConfig {
            model_path: model_path.clone(),
            language: "fr".to_string(),
            threads: Some(8),
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        let retrieved_config = transcriber.config();

        assert_eq!(retrieved_config.model_path, model_path);
        assert_eq!(retrieved_config.language, "fr");
        assert_eq!(retrieved_config.threads, Some(8));
    }

    #[test]
    fn test_whisper_transcriber_send_sync() {
        // Test that WhisperTranscriber implements Send + Sync
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WhisperTranscriber>();
        assert_sync::<WhisperTranscriber>();
    }
}
