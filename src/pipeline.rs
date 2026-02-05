//! Voice typing pipeline implementation.
//!
//! Orchestrates the complete voice-to-text flow:
//! record → transcribe → inject

use crate::audio::capture::{CpalAudioSource, suppress_audio_warnings};
use crate::audio::vad::VadConfig;
use crate::config::{Config, InputMethod};
use crate::continuous::adaptive_chunker::AdaptiveChunkerConfig;
use crate::continuous::pipeline::{ContinuousPipeline, ContinuousPipelineConfig};
use crate::error::{Result, VoicshError};
use crate::input::injector::TextInjector;
use crate::models::catalog::get_model;
use crate::models::download::{
    download_model, find_any_installed_model, is_model_installed, model_path,
};
use crate::streaming::{StreamingPipeline, StreamingPipelineConfig};
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

/// Run the record command: capture audio → transcribe → inject text.
///
/// # Arguments
/// * `config` - Base configuration (can be overridden by CLI args)
/// * `device` - Optional device override from CLI
/// * `model` - Optional model override from CLI
/// * `language` - Optional language override from CLI
/// * `quiet` - Suppress status messages
/// * `verbose` - Show detailed output (chunk progress)
/// * `no_download` - Prevent automatic model download
/// * `once` - Exit after first transcription (default: loop continuously)
/// * `chunk_size` - Chunk duration in seconds (0 = no chunking, transcribe all at once)
///
/// # Returns
/// Ok(()) on success, or an error if any step fails
#[allow(clippy::too_many_arguments)]
pub async fn run_record_command(
    mut config: Config,
    device: Option<String>,
    model: Option<String>,
    language: Option<String>,
    quiet: bool,
    verbose: bool,
    no_download: bool,
    once: bool,
    chunk_size: u32,
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
    let transcriber = Arc::new(create_transcriber(&config, quiet, no_download).await?);
    if !quiet {
        eprintln!("Ready. Listening...");
    }

    // Use continuous pipeline for default mode, streaming for --once
    if once {
        // Single recording session mode (legacy streaming pipeline)
        run_single_session(&config, transcriber, quiet, verbose, chunk_size).await
    } else {
        // Continuous mode - new pipeline that runs forever
        run_continuous(&config, transcriber, quiet, verbose).await
    }
}

/// Run the continuous pipeline until interrupted.
async fn run_continuous(
    config: &Config,
    transcriber: Arc<WhisperTranscriber>,
    quiet: bool,
    verbose: bool,
) -> Result<()> {
    // Create audio source
    let device_name = config.audio.device.as_deref();
    let audio_source = CpalAudioSource::new(device_name)?;

    // Create pipeline config
    let pipeline_config = ContinuousPipelineConfig {
        vad: VadConfig {
            speech_threshold: config.audio.vad_threshold,
            silence_duration_ms: config.audio.silence_duration_ms,
            ..Default::default()
        },
        chunker: AdaptiveChunkerConfig::default(),
        show_levels: verbose,
        auto_level: true,
        quiet,
        input_method: config.input.method.clone(),
        paste_key: config.input.paste_key.clone(),
        sample_rate: 16000,
        ..Default::default()
    };

    // Start pipeline
    let pipeline = ContinuousPipeline::new(pipeline_config);
    let handle = pipeline.start(audio_source, transcriber)?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to wait for Ctrl+C: {}", e)))?;

    if !quiet {
        eprintln!("\nShutting down...");
    }

    // Stop pipeline gracefully
    handle.stop();

    Ok(())
}

/// Run a single recording session: record → transcribe → inject.
async fn run_single_session(
    config: &Config,
    transcriber: Arc<WhisperTranscriber>,
    quiet: bool,
    verbose: bool,
    chunk_size: u32,
) -> Result<()> {
    // Create audio source
    let device_name = config.audio.device.as_deref();
    let audio_source = CpalAudioSource::new(device_name)?;

    // Configure pipeline
    let chunk_duration_ms = chunk_size * 1000;

    let pipeline_config = StreamingPipelineConfig::from_config(config)
        .with_chunk_duration_ms(chunk_duration_ms)
        .with_show_levels(verbose)
        .with_auto_level(true);

    let pipeline = StreamingPipeline::with_config(pipeline_config);

    // Run pipeline - in verbose mode show chunks as they come in
    let transcription = if verbose {
        pipeline
            .run_with_callback(audio_source, transcriber, |result| {
                // Clear level meter line before printing chunk
                eprint!("\r{:60}\r", "");
                if !result.text.is_empty() {
                    eprintln!("  > {}", result.text);
                }
            })
            .await?
    } else {
        pipeline.run(audio_source, transcriber).await?
    };

    if transcription.is_empty() {
        return Ok(());
    }

    // Show final transcription and inject
    if !quiet {
        eprintln!("\"{}\"", transcription);
    }

    inject_text(config, &transcription, false)?;

    if !quiet && verbose {
        eprintln!("  [injected]");
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

/// Inject transcribed text using configured input method.
fn inject_text(config: &Config, text: &str, verbose: bool) -> Result<()> {
    use crate::input::focused_window::resolve_paste_key;

    let injector = TextInjector::system();
    let paste_key = resolve_paste_key(&config.input.paste_key, verbose);

    match config.input.method {
        InputMethod::Clipboard => injector.inject_via_clipboard(text, paste_key),
        InputMethod::Direct => injector.inject_direct(text),
    }
}

/// Build the full path to a Whisper model file.
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
        if is_model_installed(model) {
            return Ok(model_path(model).expect("path should exist for installed model"));
        }

        return Err(VoicshError::Transcription {
            message: format!(
                "Model '{}' is not installed. Run 'voicsh models install {}' to download it.",
                model, model
            ),
        });
    }

    // Otherwise, treat as a custom model filename
    let model_filename = if model.ends_with(".bin") {
        model.to_string()
    } else {
        format!("ggml-{}.bin", model)
    };

    Ok(PathBuf::from("models").join(model_filename))
}

/// Check that required system tools are available.
fn check_prerequisites() -> Result<()> {
    // Check for wl-copy (Wayland clipboard)
    if Command::new("wl-copy").arg("--version").output().is_err() {
        return Err(VoicshError::InjectionToolNotFound {
            tool: "wl-copy".to_string(),
        });
    }

    // Test wtype
    let wtype_works = match Command::new("wtype").arg("").output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            !stderr.contains("does not support")
        }
        Err(_) => false,
    };

    // Test ydotool
    let ydotool_works = match Command::new("ydotool").args(["type", "--help"]).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            !stderr.contains("backend unavailable")
        }
        Err(_) => false,
    };

    if !wtype_works && !ydotool_works {
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
        let result = build_model_path("base.en");
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
        let path = build_model_path("custom-model").unwrap();
        assert_eq!(path, PathBuf::from("models/ggml-custom-model.bin"));
    }

    #[test]
    fn test_build_model_path_catalog_model_error_contains_install_command() {
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
