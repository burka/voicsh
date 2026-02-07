//! Daemon mode for voicsh - manages recording state and IPC server.

pub mod handler;

use crate::audio::capture::suppress_audio_warnings;
use crate::config::Config;
use crate::error::{Result, VoicshError};
use crate::ipc::server::IpcServer;
use crate::pipeline::orchestrator::PipelineHandle;
use crate::stt::transcriber::Transcriber;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(feature = "portal")]
use crate::input::portal::PortalSession;

/// Daemon state: model loaded, recording state, IPC server.
pub struct DaemonState {
    /// Configuration
    pub config: Arc<Mutex<Config>>,
    /// Loaded transcriber (model stays in memory)
    pub transcriber: Arc<dyn Transcriber>,
    /// Current pipeline handle (Some = recording, None = idle)
    pub pipeline: Arc<Mutex<Option<PipelineHandle>>>,
    /// Portal session for input injection (if available)
    #[cfg(feature = "portal")]
    pub portal: Option<Arc<PortalSession>>,
}

impl DaemonState {
    /// Creates a new daemon state with loaded model.
    ///
    /// # Arguments
    /// * `config` - Configuration
    /// * `transcriber` - Loaded transcriber (model)
    /// * `portal` - Optional portal session (feature gated)
    pub fn new(
        config: Config,
        transcriber: Arc<dyn Transcriber>,
        #[cfg(feature = "portal")] portal: Option<Arc<PortalSession>>,
    ) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            transcriber,
            pipeline: Arc::new(Mutex::new(None)),
            #[cfg(feature = "portal")]
            portal,
        }
    }

    /// Returns true if currently recording.
    pub async fn is_recording(&self) -> bool {
        self.pipeline.lock().await.is_some()
    }

    /// Returns model name from config.
    pub async fn model_name(&self) -> String {
        self.config.lock().await.stt.model.clone()
    }
}

/// Run the daemon: load model, start IPC server, wait for shutdown.
///
/// # Arguments
/// * `config` - Configuration
/// * `socket_path` - Path to Unix socket for IPC
/// * `quiet` - Suppress status messages
/// * `verbosity` - Verbosity level
/// * `no_download` - Prevent automatic model download
///
/// # Returns
/// Ok(()) on graceful shutdown, error otherwise
pub async fn run_daemon(
    config: Config,
    socket_path: Option<PathBuf>,
    quiet: bool,
    verbosity: u8,
    no_download: bool,
) -> Result<()> {
    // Suppress noisy JACK/ALSA warnings
    suppress_audio_warnings();

    // Load model once (this is slow but happens only at daemon startup)
    if !quiet {
        eprintln!("Loading model '{}'...", config.stt.model);
    }

    let transcriber = create_transcriber(&config, quiet, verbosity, no_download).await?;

    if !quiet {
        eprintln!("Model loaded successfully.");
    }

    // Try to establish portal session
    #[cfg(feature = "portal")]
    let portal = match PortalSession::try_new().await {
        Ok(session) => {
            if verbosity >= 1 {
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

    // Create daemon state
    let state = DaemonState::new(
        config,
        transcriber,
        #[cfg(feature = "portal")]
        portal,
    );

    // Determine socket path
    let socket_path = socket_path.unwrap_or_else(IpcServer::default_socket_path);

    // Create IPC server
    let server = Arc::new(IpcServer::new(socket_path)?);

    if !quiet {
        eprintln!(
            "IPC server listening at: {}",
            server.socket_path().display()
        );
        eprintln!("Daemon ready.");
    }

    // Create command handler
    let handler = handler::DaemonCommandHandler::new(state, quiet, verbosity);

    // Start IPC server in background task
    let server_clone = Arc::clone(&server);
    let server_handle = tokio::spawn(async move { server_clone.start(handler).await });

    // Wait for SIGTERM or SIGINT
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            if !quiet {
                eprintln!("\nReceived SIGINT, shutting down...");
            }
        }
        res = wait_for_sigterm() => {
            if let Err(e) = res {
                eprintln!("Error setting up signal handler: {}", e);
            }
            if !quiet {
                eprintln!("\nReceived SIGTERM, shutting down...");
            }
        }
    }

    // Stop IPC server
    server.stop().await?;

    // Wait for server task to finish
    if let Err(e) = server_handle.await {
        eprintln!("voicsh: daemon server task failed: {e}");
    }

    if !quiet {
        eprintln!("Daemon stopped.");
    }

    Ok(())
}

/// Wait for SIGTERM signal (used by systemd).
#[cfg(unix)]
async fn wait_for_sigterm() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate())
        .map_err(|e| VoicshError::Other(format!("Failed to register SIGTERM handler: {}", e)))?;
    sigterm.recv().await;
    Ok(())
}

#[cfg(not(unix))]
async fn wait_for_sigterm() -> Result<()> {
    // On non-Unix, just wait forever (Ctrl+C will still work)
    std::future::pending::<()>().await
}

/// Create transcriber from config.
async fn create_transcriber(
    config: &Config,
    quiet: bool,
    _verbosity: u8,
    no_download: bool,
) -> Result<Arc<dyn Transcriber>> {
    use crate::models::catalog::{english_variant, resolve_model_for_language};
    use crate::models::download::{
        download_model, find_any_installed_model, is_model_installed, model_path,
    };
    use crate::stt::fan_out::FanOutTranscriber;
    use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};

    let model_name = &config.stt.model;
    let language = &config.stt.language;

    // Resolve model name
    let resolved_model = resolve_model_for_language(model_name, language, quiet);

    // Check if model is installed
    if !is_model_installed(&resolved_model) {
        if no_download {
            // Try fallback to any installed model
            if let Some(fallback) = find_any_installed_model() {
                if !quiet {
                    eprintln!(
                        "Model '{}' not installed, using '{}'",
                        resolved_model, fallback
                    );
                }
            } else {
                return Err(VoicshError::TranscriptionModelNotFound {
                    path: resolved_model,
                });
            }
        } else {
            // Download model
            if !quiet {
                eprintln!("Downloading model '{}'...", resolved_model);
            }
            download_model(&resolved_model, true).await?;
        }
    }

    // Get model path
    let path = model_path(&resolved_model);

    // Get model path (must exist after download check above)
    let path = path.ok_or_else(|| VoicshError::TranscriptionModelNotFound {
        path: resolved_model.clone(),
    })?;

    // Create transcriber
    if config.stt.fan_out {
        // Fan-out mode: run English + multilingual in parallel
        let en_model = english_variant(&resolved_model).ok_or_else(|| {
            VoicshError::TranscriptionModelNotFound {
                path: format!("{}.en (English variant)", resolved_model),
            }
        })?;
        let en_path =
            model_path(en_model).ok_or_else(|| VoicshError::TranscriptionModelNotFound {
                path: en_model.to_string(),
            })?;

        if !is_model_installed(en_model) && !no_download {
            if !quiet {
                eprintln!("Downloading English model '{}'...", en_model);
            }
            download_model(en_model, true).await?;
        }

        let en_transcriber = WhisperTranscriber::new(WhisperConfig {
            model_path: en_path,
            language: "en".to_string(),
            threads: None,
        })?;

        let multilingual_transcriber = WhisperTranscriber::new(WhisperConfig {
            model_path: path,
            language: language.clone(),
            threads: None,
        })?;

        Ok(Arc::new(FanOutTranscriber::new(vec![
            Arc::new(en_transcriber) as Arc<dyn Transcriber>,
            Arc::new(multilingual_transcriber) as Arc<dyn Transcriber>,
        ])))
    } else {
        // Single model mode
        let transcriber = WhisperTranscriber::new(WhisperConfig {
            model_path: path,
            language: language.clone(),
            threads: None,
        })?;

        Ok(Arc::new(transcriber))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::MockTranscriber;

    fn mock_transcriber() -> Arc<dyn Transcriber> {
        Arc::new(MockTranscriber::new("mock-daemon-model"))
    }

    #[tokio::test]
    async fn test_daemon_state_new() {
        let config = Config::default();

        #[cfg(feature = "portal")]
        let state = DaemonState::new(config, mock_transcriber(), None);

        #[cfg(not(feature = "portal"))]
        let state = DaemonState::new(config, mock_transcriber());

        assert!(!state.is_recording().await);
    }

    #[tokio::test]
    async fn test_daemon_state_is_recording() {
        let config = Config::default();

        #[cfg(feature = "portal")]
        let state = DaemonState::new(config, mock_transcriber(), None);

        #[cfg(not(feature = "portal"))]
        let state = DaemonState::new(config, mock_transcriber());

        // Initially not recording
        assert!(!state.is_recording().await);
    }

    #[tokio::test]
    async fn test_daemon_state_model_name() {
        let mut config = Config::default();
        config.stt.model = "test-model".to_string();

        #[cfg(feature = "portal")]
        let state = DaemonState::new(config, mock_transcriber(), None);

        #[cfg(not(feature = "portal"))]
        let state = DaemonState::new(config, mock_transcriber());

        let model_name = state.model_name().await;
        assert_eq!(model_name, "test-model");
    }
}
