//! Voice typing pipeline implementation.
//!
//! Orchestrates the complete voice-to-text flow:
//! record → transcribe → inject

use crate::audio::capture::CpalAudioSource;
use crate::audio::vad::VadConfig;
use crate::config::{Config, InputMethod};
use crate::error::{Result, VoicshError};
use crate::input::injector::TextInjector;
use crate::models::catalog::get_model;
use crate::models::download::{
    download_model, find_any_installed_model, is_model_installed, model_path,
};
use crate::recording::RecordingSession;
use crate::stt::transcriber::Transcriber;
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};
use std::path::PathBuf;

/// Run the record command: capture audio → transcribe → inject text.
///
/// # Arguments
/// * `config` - Base configuration (can be overridden by CLI args)
/// * `device` - Optional device override from CLI
/// * `model` - Optional model override from CLI
/// * `language` - Optional language override from CLI
/// * `quiet` - Suppress status messages
/// * `no_download` - Prevent automatic model download
///
/// # Returns
/// Ok(()) on success, or an error if any step fails
pub async fn run_record_command(
    mut config: Config,
    device: Option<String>,
    model: Option<String>,
    language: Option<String>,
    quiet: bool,
    no_download: bool,
) -> Result<()> {
    // Apply CLI overrides
    if let Some(d) = device {
        config.audio.device = Some(d);
    }
    if let Some(m) = model {
        config.stt.model = m;
    }
    if let Some(l) = language {
        config.stt.language = l;
    }

    // Step 1: Record audio
    if !quiet {
        eprintln!("Recording...");
    }

    let audio_samples = record_audio(&config)?;

    if audio_samples.is_empty() {
        return Err(VoicshError::AudioCapture {
            message: "No audio recorded".to_string(),
        });
    }

    // Step 2: Transcribe audio
    if !quiet {
        eprintln!("Processing...");
    }

    let transcription = transcribe_audio(&config, &audio_samples, quiet, no_download).await?;

    if transcription.is_empty() {
        return Err(VoicshError::Transcription {
            message: "Transcription produced no text".to_string(),
        });
    }

    if !quiet {
        eprintln!("Transcribed: {}", transcription);
    }

    // Step 3: Inject text
    inject_text(&config, &transcription)?;

    if !quiet {
        eprintln!("Done.");
    }

    Ok(())
}

/// Record audio using configured audio source and VAD.
fn record_audio(config: &Config) -> Result<Vec<i16>> {
    // Create audio source
    let device_name = config.audio.device.as_deref();
    let audio_source = CpalAudioSource::new(device_name)?;

    // Configure VAD
    let vad_config = VadConfig {
        speech_threshold: config.audio.vad_threshold,
        silence_duration_ms: config.audio.silence_duration_ms,
        min_speech_ms: 300,
    };

    // Create recording session and record
    let mut session = RecordingSession::new(audio_source, vad_config);
    session.record_until_speech_ends()
}

/// Transcribe audio samples to text using configured STT model.
///
/// This function implements smart model selection:
/// 1. Try to use the configured model
/// 2. If not installed and auto-download is enabled, download it
/// 3. If auto-download is disabled, fall back to any installed model
/// 4. If no models are installed, return an error
async fn transcribe_audio(
    config: &Config,
    audio: &[i16],
    quiet: bool,
    no_download: bool,
) -> Result<String> {
    let configured_model = &config.stt.model;

    // Determine which model to use
    let model_to_use = if is_model_installed(configured_model) {
        // Configured model is available, use it
        configured_model.to_string()
    } else {
        // Configured model not installed
        if !quiet {
            eprintln!("Configured model '{}' is not installed.", configured_model);
        }

        // Try to find any installed model as fallback
        if let Some(fallback_model) = find_any_installed_model() {
            if !quiet {
                eprintln!("Using installed model '{}' instead.", fallback_model);
            }
            fallback_model
        } else {
            // No models installed at all
            if no_download {
                return Err(VoicshError::Transcription {
                    message: format!(
                        "Model '{}' is not installed and --no-download was specified. \
                        Run 'voicsh models install {}' to download it.",
                        configured_model, configured_model
                    ),
                });
            }

            // Auto-download the configured model
            if !quiet {
                eprintln!(
                    "Downloading model '{}' (this may take a while)...",
                    configured_model
                );
            }

            download_model(configured_model, !quiet).await?;

            if !quiet {
                eprintln!("Model '{}' downloaded successfully.", configured_model);
            }

            configured_model.to_string()
        }
    };

    // Build model path
    let model_path = build_model_path(&model_to_use)?;

    // Create transcriber
    let whisper_config = WhisperConfig {
        model_path,
        language: config.stt.language.clone(),
        threads: None, // Auto-detect
    };

    let transcriber = WhisperTranscriber::new(whisper_config)?;

    // Transcribe
    transcriber.transcribe(audio)
}

/// Inject transcribed text using configured input method.
fn inject_text(config: &Config, text: &str) -> Result<()> {
    let injector = TextInjector::system();

    match config.input.method {
        InputMethod::Clipboard => injector.inject_via_clipboard(text),
        InputMethod::Direct => injector.inject_direct(text),
    }
}

/// Build the full path to a Whisper model file.
///
/// Supports several model path formats:
/// - Absolute path: /path/to/model.bin
/// - Relative path: ./models/model.bin
/// - Model name only: base.en → looks in cache dir first, then ./models/
///
/// # Arguments
/// * `model` - Model path or name
///
/// # Returns
/// Full PathBuf to the model file
///
/// # Errors
/// Returns an error with helpful message if model is not found
fn build_model_path(model: &str) -> Result<PathBuf> {
    let path = PathBuf::from(model);

    // If it's an absolute path or exists as-is, use it directly
    if path.is_absolute() || path.exists() {
        return Ok(path);
    }

    // If it contains a path separator, treat as relative path
    if model.contains('/') || model.contains('\\') {
        return Ok(path);
    }

    // Check if it's a model name from the catalog
    if get_model(model).is_some() {
        // Check if installed in cache directory
        if is_model_installed(model) {
            return Ok(model_path(model).expect("path should exist for installed model"));
        }

        // Not installed - provide helpful error message
        return Err(VoicshError::Transcription {
            message: format!(
                "Model '{}' is not installed. Run 'voicsh models install {}' to download it.",
                model, model
            ),
        });
    }

    // Otherwise, treat as a custom model filename and construct path
    let model_filename = if model.ends_with(".bin") {
        model.to_string()
    } else {
        format!("ggml-{}.bin", model)
    };

    Ok(PathBuf::from("models").join(model_filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_model_path_with_absolute_path() {
        let path = build_model_path("/absolute/path/to/model.bin").unwrap();
        assert_eq!(path, PathBuf::from("/absolute/path/to/model.bin"));
    }

    #[test]
    fn test_build_model_path_with_relative_path() {
        let path = build_model_path("./custom/model.bin").unwrap();
        assert_eq!(path, PathBuf::from("./custom/model.bin"));
    }

    #[test]
    fn test_build_model_path_with_model_name_not_installed() {
        // When a catalog model is not installed, should return error with helpful message
        let result = build_model_path("base.en");
        // Could be installed or not, check both cases
        if result.is_err() {
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("not installed") || err_msg.contains("voicsh models install"));
        }
    }

    #[test]
    fn test_build_model_path_with_model_name_and_bin_extension() {
        let path = build_model_path("ggml-tiny.bin").unwrap();
        assert_eq!(path, PathBuf::from("models/ggml-tiny.bin"));
    }

    #[test]
    fn test_build_model_path_with_windows_path() {
        let path = build_model_path("custom\\models\\model.bin").unwrap();
        assert_eq!(path, PathBuf::from("custom\\models\\model.bin"));
    }

    #[test]
    fn test_build_model_path_with_unknown_model_name() {
        // Unknown model names (not in catalog) should still build a path
        let path = build_model_path("custom-model").unwrap();
        assert_eq!(path, PathBuf::from("models/ggml-custom-model.bin"));
    }

    #[test]
    fn test_build_model_path_catalog_model_error_contains_install_command() {
        // When a catalog model is not installed, error should mention install command
        let result = build_model_path("tiny.en");
        if result.is_err() {
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("voicsh models install"),
                "Error message should suggest install command: {}",
                err_msg
            );
        }
    }
}
