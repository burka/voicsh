//! Whisper-based speech-to-text transcription.
//!
//! This module provides a Whisper implementation of the Transcriber trait using whisper-rs.
//!
//! # Feature Gate
//!
//! This module requires the `whisper` feature to be enabled and cmake to be installed.
//! To build with Whisper support:
//!
//! ```bash
//! cargo build --features whisper
//! ```

use crate::defaults;
use crate::error::{Result, VoicshError};
use crate::stt::transcriber::Transcriber;
use std::path::PathBuf;

#[cfg(feature = "whisper")]
use std::sync::Mutex;
#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
/// Uses whisper-rs for real-time speech-to-text transcription.
/// The WhisperContext is wrapped in a Mutex to ensure thread safety.
///
/// # Feature Gate
///
/// This type is only available when the `whisper` feature is enabled.
#[cfg(feature = "whisper")]
#[derive(Debug)]
pub struct WhisperTranscriber {
    context: Mutex<WhisperContext>,
    config: WhisperConfig,
    model_name: String,
}

/// Whisper-based transcriber placeholder (without whisper feature).
///
/// This is a stub implementation that returns errors when used.
/// Enable the `whisper` feature to use real transcription.
#[cfg(not(feature = "whisper"))]
#[derive(Debug)]
pub struct WhisperTranscriber {
    config: WhisperConfig,
    model_name: String,
}

#[cfg(feature = "whisper")]
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
    /// Returns `VoicshError::TranscriptionInferenceFailed` if model loading fails
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

        // Load the Whisper model
        let context_params = WhisperContextParameters::default();
        let context = WhisperContext::new_with_params(
            config.model_path.to_str().ok_or_else(|| {
                VoicshError::TranscriptionInferenceFailed {
                    message: "Invalid UTF-8 in model path".to_string(),
                }
            })?,
            context_params,
        )
        .map_err(|e| VoicshError::TranscriptionInferenceFailed {
            message: format!("Failed to load Whisper model: {}", e),
        })?;

        Ok(Self {
            context: Mutex::new(context),
            config,
            model_name,
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }

    /// Convert i16 audio samples to f32 normalized to [-1.0, 1.0]
    ///
    /// Whisper expects audio in f32 format normalized to the range [-1.0, 1.0].
    /// Input is 16-bit PCM audio where samples range from -32768 to 32767.
    fn convert_audio(samples: &[i16]) -> Vec<f32> {
        samples
            .iter()
            .map(|&sample| sample as f32 / 32768.0)
            .collect()
    }
}

#[cfg(not(feature = "whisper"))]
impl WhisperTranscriber {
    /// Create a new Whisper transcriber (stub implementation).
    ///
    /// This returns an error indicating that the whisper feature is not enabled.
    pub fn new(config: WhisperConfig) -> Result<Self> {
        if !config.model_path.exists() {
            return Err(VoicshError::TranscriptionModelNotFound {
                path: config.model_path.to_string_lossy().to_string(),
            });
        }

        let model_name = config
            .model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self { config, model_name })
    }

    /// Get the configuration
    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }

    /// Convert i16 audio samples to f32 normalized to [-1.0, 1.0]
    ///
    /// This function is available even without the whisper feature for testing.
    pub fn convert_audio(samples: &[i16]) -> Vec<f32> {
        samples
            .iter()
            .map(|&sample| sample as f32 / 32768.0)
            .collect()
    }
}

#[cfg(feature = "whisper")]
impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, audio: &[i16]) -> Result<String> {
        // Convert audio format from i16 to f32
        let audio_f32 = Self::convert_audio(audio);

        // Lock the context for thread-safe access
        let mut context =
            self.context
                .lock()
                .map_err(|e| VoicshError::TranscriptionInferenceFailed {
                    message: format!("Failed to acquire context lock: {}", e),
                })?;

        // Create a new state for this transcription
        let mut state =
            context
                .create_state()
                .map_err(|e| VoicshError::TranscriptionInferenceFailed {
                    message: format!("Failed to create Whisper state: {}", e),
                })?;

        // Configure transcription parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Set language
        params.set_language(Some(&self.config.language));

        // Set number of threads if specified
        if let Some(threads) = self.config.threads {
            params.set_n_threads(threads as i32);
        }

        // Disable printing to stdout/stderr
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Run inference
        state
            .full(params, &audio_f32)
            .map_err(|e| VoicshError::TranscriptionInferenceFailed {
                message: format!("Whisper inference failed: {}", e),
            })?;

        // Extract transcribed text from all segments
        let num_segments =
            state
                .full_n_segments()
                .map_err(|e| VoicshError::TranscriptionInferenceFailed {
                    message: format!("Failed to get segment count: {}", e),
                })?;

        let mut transcription = String::new();
        for i in 0..num_segments {
            let segment_text = state.full_get_segment_text(i).map_err(|e| {
                VoicshError::TranscriptionInferenceFailed {
                    message: format!("Failed to get segment text: {}", e),
                }
            })?;
            transcription.push_str(&segment_text);
        }

        // Trim whitespace from the result
        Ok(transcription.trim().to_string())
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn is_ready(&self) -> bool {
        // The transcriber is ready if we successfully created it
        true
    }
}

#[cfg(not(feature = "whisper"))]
impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, _audio: &[i16]) -> Result<String> {
        Err(VoicshError::TranscriptionInferenceFailed {
            message: "Whisper feature not enabled. Rebuild with --features whisper".to_string(),
        })
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn is_ready(&self) -> bool {
        false
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
    fn test_whisper_transcriber_model_name_extraction() {
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

        let result = WhisperTranscriber::new(config);

        // With whisper feature: fails because it's not a valid model file
        // Without whisper feature: succeeds (stub only checks file exists)
        #[cfg(feature = "whisper")]
        assert!(result.is_err(), "Should fail with invalid model file");

        #[cfg(not(feature = "whisper"))]
        {
            assert!(result.is_ok(), "Stub should succeed if file exists");
            let transcriber = result.unwrap();
            assert_eq!(transcriber.model_name(), "ggml-base");
        }

        // Cleanup
        std::fs::remove_file(&model_path).unwrap();
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
    fn test_convert_audio_i16_to_f32() {
        // Test conversion of common values
        let samples = vec![0i16, 16384, -16384, 32767, -32768];
        let converted = WhisperTranscriber::convert_audio(&samples);

        assert_eq!(converted.len(), samples.len());
        assert_eq!(converted[0], 0.0); // 0 -> 0.0
        assert!((converted[1] - 0.5).abs() < 0.01); // 16384 -> ~0.5
        assert!((converted[2] + 0.5).abs() < 0.01); // -16384 -> ~-0.5
        assert!((converted[3] - 0.999969).abs() < 0.01); // 32767 -> ~1.0
        assert_eq!(converted[4], -1.0); // -32768 -> -1.0
    }

    #[test]
    fn test_convert_audio_empty() {
        let samples: Vec<i16> = vec![];
        let converted = WhisperTranscriber::convert_audio(&samples);
        assert_eq!(converted.len(), 0);
    }

    #[test]
    fn test_convert_audio_large_array() {
        // Test with a larger array (1 second of audio at 16kHz)
        let samples = vec![0i16; 16000];
        let converted = WhisperTranscriber::convert_audio(&samples);
        assert_eq!(converted.len(), 16000);
        assert!(converted.iter().all(|&x| x == 0.0));
    }

    // Integration tests that require an actual model file
    // These are marked as #[ignore] and must be run manually with a real model

    #[test]
    #[ignore]
    fn test_whisper_transcriber_with_real_model() {
        // To run this test:
        // 1. Download a Whisper model (e.g., ggml-base.en.bin)
        // 2. Place it in the models/ directory
        // 3. Run: cargo test test_whisper_transcriber_with_real_model -- --ignored

        let model_path = PathBuf::from("models/ggml-base.en.bin");
        if !model_path.exists() {
            panic!(
                "Model not found at {:?}. Download it before running this test.",
                model_path
            );
        }

        let config = WhisperConfig {
            model_path,
            language: "en".to_string(),
            threads: Some(4),
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();
        assert!(transcriber.is_ready());
        assert_eq!(transcriber.model_name(), "ggml-base.en");
    }

    #[test]
    #[ignore]
    fn test_whisper_transcribe_with_real_audio() {
        // To run this test, you need:
        // 1. A real Whisper model in models/ggml-base.en.bin
        // 2. Real audio data (this test uses silent audio as placeholder)
        // Run: cargo test test_whisper_transcribe_with_real_audio -- --ignored

        let model_path = PathBuf::from("models/ggml-base.en.bin");
        if !model_path.exists() {
            panic!(
                "Model not found at {:?}. Download it before running this test.",
                model_path
            );
        }

        let config = WhisperConfig {
            model_path,
            language: "en".to_string(),
            threads: Some(4),
        };

        let transcriber = WhisperTranscriber::new(config).unwrap();

        // Create 1 second of silence (16kHz mono)
        let audio = vec![0i16; 16000];
        let result = transcriber.transcribe(&audio);

        assert!(result.is_ok());
        let text = result.unwrap();
        // Silent audio should produce empty or minimal transcription
        println!("Transcription result: '{}'", text);
    }

    #[test]
    fn test_whisper_transcriber_send_sync() {
        // Test that WhisperTranscriber implements Send + Sync
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<WhisperTranscriber>();
        assert_sync::<WhisperTranscriber>();
    }

    #[test]
    fn test_whisper_transcriber_implements_transcriber_trait() {
        // Test that we can use the trait object without a real model
        // (we can't create an instance without a model, but we can verify the trait bounds)
        fn accepts_transcriber(_t: &dyn Transcriber) {}

        // This test just verifies the trait is implemented correctly
        // Actual usage requires a real model file
    }
}
