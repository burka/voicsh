//! Voice typing application entry point.
//!
//! Orchestrates the complete voice-to-text flow:
//! record → transcribe → inject

use crate::audio::capture::{CpalAudioSource, suppress_audio_warnings};
use crate::audio::recorder::AudioSource;
use crate::audio::vad::VadConfig;
use crate::config::Config;
use crate::error::{Result, VoicshError};
use crate::input::injector::SystemCommandExecutor;
use crate::models::catalog::{english_variant, get_model, multilingual_variant};
use crate::models::download::{
    download_model, find_any_installed_model, is_model_installed, model_path,
};
use crate::pipeline::adaptive_chunker::AdaptiveChunkerConfig;
use crate::pipeline::orchestrator::{Pipeline, PipelineConfig};
use crate::pipeline::sink::{CollectorSink, InjectorSink};
use crate::stt::fan_out::FanOutTranscriber;
use crate::stt::transcriber::Transcriber;
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

#[cfg(feature = "portal")]
use crate::input::portal::PortalSession;

/// Run the record command: capture audio → transcribe → inject text.
///
/// # Arguments
/// * `config` - Base configuration (can be overridden by CLI args)
/// * `device` - Optional device override from CLI
/// * `model` - Optional model override from CLI
/// * `language` - Optional language override from CLI
/// * `quiet` - Suppress status messages
/// * `verbosity` - Verbosity level (0=default, 1=meter+results, 2=full diagnostics)
/// * `no_download` - Prevent automatic model download
/// * `once` - Exit after first transcription (default: loop continuously)
/// * `fan_out` - Run English + multilingual models in parallel
/// * `chunk_size` - Chunk duration in seconds (unused for now, reserved)
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
    verbosity: u8,
    no_download: bool,
    once: bool,
    fan_out: bool,
    _chunk_size: u32,
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
    if fan_out {
        config.stt.fan_out = true;
    }

    // Try to establish portal session for key injection (GNOME/KDE)
    #[cfg(feature = "portal")]
    let portal = match PortalSession::try_new().await {
        Ok(session) => {
            if !quiet {
                eprintln!("Portal keyboard access granted.");
            }
            Some(Arc::new(session))
        }
        Err(e) => {
            if verbosity >= 2 {
                eprintln!("Portal unavailable ({}), using wtype/ydotool fallback.", e);
            }
            None
        }
    };

    // Load model ONCE before the loop (this is the slow part)
    if !quiet {
        eprintln!("Loading model '{}'...", config.stt.model);
    }
    let transcriber: Arc<dyn Transcriber> = create_transcriber(&config, quiet, no_download).await?;
    if !quiet {
        eprintln!("Ready. Listening...");
    }

    #[cfg(feature = "portal")]
    let make_sink = |config: &Config| {
        InjectorSink::with_portal(
            config.input.method.clone(),
            config.input.paste_key.clone(),
            verbosity,
            portal.clone(),
        )
    };
    #[cfg(not(feature = "portal"))]
    let make_sink = |config: &Config| {
        InjectorSink::system(
            config.input.method.clone(),
            config.input.paste_key.clone(),
            verbosity,
        )
    };

    if once {
        run_single_session(&config, transcriber, quiet, verbosity, make_sink).await
    } else {
        run_continuous(&config, transcriber, quiet, verbosity, make_sink).await
    }
}

/// Run the continuous pipeline until interrupted.
async fn run_continuous(
    config: &Config,
    transcriber: Arc<dyn Transcriber>,
    quiet: bool,
    verbosity: u8,
    make_sink: impl FnOnce(&Config) -> InjectorSink<SystemCommandExecutor>,
) -> Result<()> {
    let device_name = config.audio.device.as_deref();
    let audio_source: Box<dyn AudioSource> = Box::new(CpalAudioSource::new(device_name)?);

    let pipeline_config = PipelineConfig {
        vad: VadConfig {
            speech_threshold: config.audio.vad_threshold,
            silence_duration_ms: config.audio.silence_duration_ms,
            ..Default::default()
        },
        chunker: AdaptiveChunkerConfig::default(),
        verbosity,
        auto_level: true,
        quiet,
        sample_rate: 16000,
        ..Default::default()
    };

    let sink = make_sink(config);

    let pipeline = Pipeline::new(pipeline_config);
    let handle = pipeline.start(audio_source, transcriber, Box::new(sink))?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to wait for Ctrl+C: {}", e)))?;

    if !quiet {
        eprintln!("\nShutting down...");
    }

    handle.stop();
    Ok(())
}

/// Run a single recording session using CollectorSink.
///
/// Records until Ctrl+C, collects all transcriptions, then injects the result.
async fn run_single_session(
    config: &Config,
    transcriber: Arc<dyn Transcriber>,
    quiet: bool,
    verbosity: u8,
    make_sink: impl FnOnce(&Config) -> InjectorSink<SystemCommandExecutor>,
) -> Result<()> {
    let device_name = config.audio.device.as_deref();
    let audio_source: Box<dyn AudioSource> = Box::new(CpalAudioSource::new(device_name)?);

    let pipeline_config = PipelineConfig {
        vad: VadConfig {
            speech_threshold: config.audio.vad_threshold,
            silence_duration_ms: config.audio.silence_duration_ms,
            ..Default::default()
        },
        chunker: AdaptiveChunkerConfig::default(),
        verbosity,
        auto_level: true,
        quiet,
        sample_rate: 16000,
        ..Default::default()
    };

    let sink = CollectorSink::new();
    let pipeline = Pipeline::new(pipeline_config);
    let handle = pipeline.start(audio_source, transcriber, Box::new(sink))?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to wait for Ctrl+C: {}", e)))?;

    if !quiet {
        eprintln!("\nProcessing...");
    }

    // Stop pipeline and get collected text
    let transcription = handle.stop();

    if let Some(text) = transcription
        && !text.is_empty()
    {
        if !quiet {
            eprintln!("\"{}\"", text);
        }
        // Use the same sink factory to get portal-aware injection
        let mut injector_sink = make_sink(config);
        use crate::pipeline::sink::TextSink;
        injector_sink.handle(&text)?;
        if !quiet && verbosity >= 2 {
            eprintln!("  [injected]");
        }
    }

    Ok(())
}

/// Resolve the model name based on the configured language.
///
/// Ensures a multilingual model is used when language is not English.
/// - `language="auto"` + `model="base.en"` → switch to `"base"`, warn
/// - `language="de"` + `model="base.en"` → switch to `"base"`, warn
/// - `language="en"` + `model="base.en"` → keep as-is
/// - `language="auto"` + `model="base"` → keep as-is
fn resolve_model_for_language(model: &str, language: &str, quiet: bool) -> String {
    let needs_multilingual = language == "auto" || (language != "en" && !language.is_empty());
    let is_english_only = model.ends_with(".en");

    if needs_multilingual
        && is_english_only
        && let Some(ml) = multilingual_variant(model)
    {
        if !quiet {
            eprintln!(
                "Switching model '{}' → '{}' (language='{}' needs multilingual model).",
                model, ml, language
            );
        }
        return ml.to_string();
    }
    model.to_string()
}

/// Create the transcriber, handling model download and fan-out if needed.
async fn create_transcriber(
    config: &Config,
    quiet: bool,
    no_download: bool,
) -> Result<Arc<dyn Transcriber>> {
    let resolved_model = resolve_model_for_language(&config.stt.model, &config.stt.language, quiet);

    // Fan-out: run multilingual + English models in parallel
    if config.stt.fan_out
        && config.stt.language == "auto"
        && let Some(en) = english_variant(&resolved_model)
        && en != resolved_model
    {
        let ml =
            load_single_model(&resolved_model, &config.stt.language, quiet, no_download).await?;
        let en = load_single_model(en, "en", quiet, no_download).await?;
        if !quiet {
            eprintln!("Fan-out: {} + {}", ml.model_name(), en.model_name());
        }
        return Ok(Arc::new(FanOutTranscriber::new(vec![
            Arc::new(ml) as Arc<dyn Transcriber>,
            Arc::new(en) as Arc<dyn Transcriber>,
        ])));
    }

    let transcriber =
        load_single_model(&resolved_model, &config.stt.language, quiet, no_download).await?;
    Ok(Arc::new(transcriber))
}

/// Load a single Whisper model, downloading if needed.
async fn load_single_model(
    model_name: &str,
    language: &str,
    quiet: bool,
    no_download: bool,
) -> Result<WhisperTranscriber> {
    let model_to_use = if is_model_installed(model_name) {
        model_name.to_string()
    } else if no_download {
        if let Some(fallback) = find_any_installed_model() {
            if !quiet {
                eprintln!(
                    "Model '{}' not installed (--no-download). Using '{}'.",
                    model_name, fallback
                );
            }
            fallback
        } else {
            return Err(VoicshError::Transcription {
                message: format!(
                    "Model '{}' not installed and --no-download specified.\n\
                     Run: voicsh models install {}",
                    model_name, model_name
                ),
            });
        }
    } else {
        if !quiet {
            eprintln!("Downloading model '{}'...", model_name);
        }
        download_model(model_name, !quiet).await?;
        if !quiet {
            eprintln!("Download complete.");
        }
        model_name.to_string()
    };

    let model_path = build_model_path(&model_to_use)?;
    let whisper_config = WhisperConfig {
        model_path,
        language: language.to_string(),
        threads: None,
    };

    WhisperTranscriber::new(whisper_config)
}

/// Build the full path to a Whisper model file.
fn build_model_path(model: &str) -> Result<PathBuf> {
    let path = PathBuf::from(model);

    if path.is_absolute() || path.exists() {
        return Ok(path);
    }

    if model.contains('/') || model.contains('\\') {
        return Ok(path);
    }

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

    let model_filename = if model.ends_with(".bin") {
        model.to_string()
    } else {
        format!("ggml-{}.bin", model)
    };

    Ok(PathBuf::from("models").join(model_filename))
}

/// Check that required system tools are available.
fn check_prerequisites() -> Result<()> {
    if Command::new("wl-copy").arg("--version").output().is_err() {
        return Err(VoicshError::InjectionToolNotFound {
            tool: "wl-copy".to_string(),
        });
    }

    let wtype_works = match Command::new("wtype").arg("").output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            !stderr.contains("does not support")
        }
        Err(_) => false,
    };

    let ydotool_works = match Command::new("ydotool").args(["type", "--help"]).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            !stderr.contains("backend unavailable")
        }
        Err(_) => false,
    };

    if !wtype_works && !ydotool_works {
        // With the portal feature, wtype/ydotool absence is not fatal -
        // the portal may provide key injection at runtime.
        #[cfg(feature = "portal")]
        {
            eprintln!(
                "Warning: Neither wtype nor ydotool available.\n\
                 Will attempt xdg-desktop-portal RemoteDesktop for key injection."
            );
            return Ok(());
        }

        #[cfg(not(feature = "portal"))]
        {
            let ydotool_installed = Command::new("ydotool").arg("--version").output().is_ok()
                || Command::new("ydotool")
                    .arg("type")
                    .arg("--help")
                    .output()
                    .is_ok();

            let wtype_installed = Command::new("wtype").arg("--help").output().is_ok();

            let mut msg = String::from("Text injection not available:\n");

            if wtype_installed {
                msg.push_str(
                    "  - wtype: installed but compositor doesn't support virtual keyboard\n",
                );
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

    #[test]
    fn test_resolve_auto_with_english_model_switches_to_multilingual() {
        let result = resolve_model_for_language("base.en", "auto", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_non_english_with_english_model_switches() {
        let result = resolve_model_for_language("base.en", "de", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_english_with_english_model_keeps() {
        let result = resolve_model_for_language("base.en", "en", true);
        assert_eq!(result, "base.en");
    }

    #[test]
    fn test_resolve_auto_with_multilingual_model_keeps() {
        let result = resolve_model_for_language("base", "auto", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_unknown_model_keeps_as_is() {
        let result = resolve_model_for_language("custom-model.en", "auto", true);
        // Unknown model, no catalog entry, keep as-is
        assert_eq!(result, "custom-model.en");
    }
}
