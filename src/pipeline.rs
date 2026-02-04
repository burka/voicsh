//! Voice typing pipeline implementation.
//!
//! Orchestrates the complete voice-to-text flow:
//! record → transcribe → inject

use crate::audio::capture::{CpalAudioSource, suppress_audio_warnings};
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
use std::process::Command;

/// Run the record command: capture audio → transcribe → inject text.
///
/// # Arguments
/// * `config` - Base configuration (can be overridden by CLI args)
/// * `device` - Optional device override from CLI
/// * `model` - Optional model override from CLI
/// * `language` - Optional language override from CLI
/// * `quiet` - Suppress status messages
/// * `no_download` - Prevent automatic model download
/// * `once` - Exit after first transcription (default: loop continuously)
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
    once: bool,
) -> Result<()> {
    // Suppress noisy JACK/ALSA warnings before audio init
    suppress_audio_warnings();

    // Check prerequisites first (before any heavy work)
    check_prerequisites()?;

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

    // Load model ONCE before the loop (this is the slow part)
    if !quiet {
        eprintln!("Loading model '{}'...", config.stt.model);
    }
    let transcriber = create_transcriber(&config, quiet, no_download).await?;
    if !quiet {
        eprintln!("Model loaded. Ready!");
    }

    // Loop until user interrupts (or once=true for single run)
    let mut iteration = 0;
    loop {
        iteration += 1;

        // Print separator after first iteration
        if iteration > 1 && !quiet {
            eprintln!("\n--- Recording {} ---", iteration);
        }

        // Step 1: Record audio (show level meter if not quiet)
        let audio_samples = record_audio(&config, !quiet)?;

        if audio_samples.is_empty() {
            if !quiet {
                eprintln!("No audio recorded, skipping...");
            }
            if once {
                break;
            }
            continue;
        }

        // Step 2: Transcribe audio (model already loaded - this is fast)
        if !quiet {
            eprintln!("Transcribing...");
        }

        let transcription = transcriber.transcribe(&audio_samples)?;

        if transcription.is_empty() {
            if !quiet {
                eprintln!("No speech detected, skipping...");
            }
            if once {
                break;
            }
            continue;
        }

        if !quiet {
            eprintln!("Transcribed: {}", transcription);
        }

        // Step 3: Inject text
        match inject_text(&config, &transcription) {
            Ok(()) => {
                if !quiet {
                    eprintln!("Done.");
                }
            }
            Err(e) => {
                eprintln!("Error injecting text: {}", e);
                if once {
                    return Err(e);
                }
                // Continue to next iteration in loop mode
            }
        }

        // Exit after first iteration if once flag is set
        if once {
            break;
        }
    }

    Ok(())
}

/// Create the transcriber, handling model download if needed.
async fn create_transcriber(
    config: &Config,
    quiet: bool,
    no_download: bool,
) -> Result<WhisperTranscriber> {
    let configured_model = &config.stt.model;

    // Determine which model to use
    let model_to_use = if is_model_installed(configured_model) {
        // Requested model is available
        configured_model.to_string()
    } else if no_download {
        // Can't download, try fallback
        if let Some(fallback) = find_any_installed_model() {
            if !quiet {
                eprintln!(
                    "Model '{}' not installed (--no-download). Using '{}'.",
                    configured_model, fallback
                );
            }
            fallback
        } else {
            return Err(VoicshError::Transcription {
                message: format!(
                    "Model '{}' not installed and --no-download specified.\n\
                     Run: voicsh models install {}",
                    configured_model, configured_model
                ),
            });
        }
    } else {
        // Auto-download the requested model
        if !quiet {
            eprintln!("Downloading model '{}'...", configured_model);
        }
        download_model(configured_model, !quiet).await?;
        if !quiet {
            eprintln!("Download complete.");
        }
        configured_model.to_string()
    };

    let model_path = build_model_path(&model_to_use)?;
    let whisper_config = WhisperConfig {
        model_path,
        language: config.stt.language.clone(),
        threads: None,
    };

    WhisperTranscriber::new(whisper_config)
}

/// Record audio using configured audio source and VAD.
fn record_audio(config: &Config, show_levels: bool) -> Result<Vec<i16>> {
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
    let mut session =
        RecordingSession::new(audio_source, vad_config).with_level_display(show_levels);
    session.record_until_speech_ends()
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

/// Check that required system tools are available.
///
/// Returns an error early if critical dependencies are missing,
/// so the user doesn't wait through recording/transcription only to fail at injection.
fn check_prerequisites() -> Result<()> {
    // Check for wl-copy (Wayland clipboard)
    if Command::new("wl-copy").arg("--version").output().is_err() {
        return Err(VoicshError::InjectionToolNotFound {
            tool: "wl-copy".to_string(),
        });
    }

    // Test wtype by sending an empty key sequence (tests compositor support)
    // wtype fails with "Compositor does not support virtual keyboard" if unsupported
    let wtype_works = match Command::new("wtype").arg("").output() {
        Ok(output) => {
            // wtype returns non-zero and prints error if compositor doesn't support it
            let stderr = String::from_utf8_lossy(&output.stderr);
            !stderr.contains("does not support")
        }
        Err(_) => false,
    };

    // Test ydotool - check if backend is available by examining stderr
    let ydotool_works = match Command::new("ydotool").args(["type", "--help"]).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // ydotool works if it doesn't report "backend unavailable"
            !stderr.contains("backend unavailable")
        }
        Err(_) => false,
    };

    if !wtype_works && !ydotool_works {
        // Neither tool works - provide detailed error
        let ydotool_installed = Command::new("ydotool").arg("--version").output().is_ok()
            || Command::new("ydotool")
                .arg("type")
                .arg("--help")
                .output()
                .is_ok();

        let wtype_installed = Command::new("wtype").arg("--help").output().is_ok();

        let mut msg = String::from("Text injection not available:\n");

        if wtype_installed {
            msg.push_str("  - wtype: installed but compositor doesn't support virtual keyboard\n");
        } else {
            msg.push_str("  - wtype: not installed\n");
        }

        if ydotool_installed {
            msg.push_str("  - ydotool: installed but ydotoold daemon not running\n\n");
            msg.push_str("Fix: Start ydotoold daemon: sudo ydotoold &\n");
            msg.push_str("  Or: systemctl --user enable --now ydotool (if available)");
        } else {
            msg.push_str("  - ydotool: not installed\n\n");
            msg.push_str("Install one of:\n");
            msg.push_str("  sudo apt install wtype  (for Sway/wlroots compositors)\n");
            msg.push_str("  sudo apt install ydotool  (then start ydotoold daemon)");
        }

        return Err(VoicshError::InjectionFailed { message: msg });
    }

    Ok(())
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
