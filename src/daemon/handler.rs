//! Command handler implementation for the daemon.

use crate::audio::capture::CpalAudioSource;
use crate::audio::recorder::AudioSource;
use crate::audio::vad::VadConfig;
use crate::daemon::DaemonState;
use crate::input::injector::{InjectorSink, SystemCommandExecutor};
use crate::ipc::protocol::{Command, Response};
use crate::ipc::server::CommandHandler;
use crate::pipeline::adaptive_chunker::AdaptiveChunkerConfig;
use crate::pipeline::orchestrator::{Pipeline, PipelineConfig};
use crate::pipeline::sink::CollectorSink;
use std::sync::Arc;

#[cfg(feature = "portal")]
use crate::input::portal::PortalSession;

/// Command handler for daemon IPC commands.
pub struct DaemonCommandHandler {
    state: Arc<DaemonState>,
    quiet: bool,
    verbosity: u8,
}

impl DaemonCommandHandler {
    /// Creates a new command handler.
    pub fn new(state: DaemonState, quiet: bool, verbosity: u8) -> Self {
        Self {
            state: Arc::new(state),
            quiet,
            verbosity,
        }
    }

    /// Start recording.
    async fn start_recording(&self) -> Response {
        // Check if already recording
        if self.state.is_recording().await {
            return Response::Error {
                message: "Already recording".to_string(),
            };
        }

        // Get config
        let config = self.state.config.lock().await.clone();

        // Create audio source
        let device_name = config.audio.device.as_deref();
        let audio_source: Box<dyn AudioSource> = match CpalAudioSource::new(device_name) {
            Ok(source) => Box::new(source),
            Err(e) => {
                return Response::Error {
                    message: format!("Failed to create audio source: {}", e),
                };
            }
        };

        // Create pipeline config
        let pipeline_config = PipelineConfig {
            vad: VadConfig {
                speech_threshold: config.audio.vad_threshold,
                silence_duration_ms: config.audio.silence_duration_ms,
                ..Default::default()
            },
            chunker: AdaptiveChunkerConfig::default(),
            verbosity: self.verbosity,
            auto_level: true,
            quiet: self.quiet,
            sample_rate: 16000,
            ..Default::default()
        };

        // Create sink with portal support
        #[cfg(feature = "portal")]
        let sink = InjectorSink::with_portal(
            config.input.method.clone(),
            config.input.paste_key.clone(),
            self.verbosity,
            self.state.portal.clone(),
        );

        #[cfg(not(feature = "portal"))]
        let sink = InjectorSink::system(
            config.input.method.clone(),
            config.input.paste_key.clone(),
            self.verbosity,
        );

        // Start pipeline
        let transcriber = Arc::clone(&self.state.transcriber);
        let pipeline = Pipeline::new(pipeline_config);

        match pipeline.start(audio_source, transcriber, Box::new(sink)) {
            Ok(handle) => {
                *self.state.pipeline.lock().await = Some(handle);
                Response::Ok
            }
            Err(e) => Response::Error {
                message: format!("Failed to start pipeline: {}", e),
            },
        }
    }

    /// Stop recording and return transcription.
    async fn stop_recording(&self) -> Response {
        // Check if recording
        let mut pipeline_guard = self.state.pipeline.lock().await;

        if let Some(handle) = pipeline_guard.take() {
            // Stop pipeline and get result
            let result = handle.stop();

            if let Some(text) = result {
                Response::Transcription { text }
            } else {
                Response::Ok
            }
        } else {
            Response::Error {
                message: "Not recording".to_string(),
            }
        }
    }

    /// Cancel recording without transcription.
    async fn cancel_recording(&self) -> Response {
        // Check if recording
        let mut pipeline_guard = self.state.pipeline.lock().await;

        if let Some(handle) = pipeline_guard.take() {
            // Just drop the handle to stop pipeline
            drop(handle);
            Response::Ok
        } else {
            Response::Error {
                message: "Not recording".to_string(),
            }
        }
    }

    /// Toggle recording on/off.
    async fn toggle_recording(&self) -> Response {
        if self.state.is_recording().await {
            self.stop_recording().await
        } else {
            self.start_recording().await
        }
    }

    /// Get daemon status.
    async fn get_status(&self) -> Response {
        let recording = self.state.is_recording().await;
        let model_name = Some(self.state.model_name().await);

        Response::Status {
            recording,
            model_loaded: true, // Model is always loaded in daemon
            model_name,
        }
    }
}

#[async_trait::async_trait]
impl CommandHandler for DaemonCommandHandler {
    async fn handle(&self, command: Command) -> Response {
        match command {
            Command::Start => self.start_recording().await,
            Command::Stop => self.stop_recording().await,
            Command::Cancel => self.cancel_recording().await,
            Command::Toggle => self.toggle_recording().await,
            Command::Status => self.get_status().await,
            Command::Shutdown => {
                // Shutdown is handled by stopping the IPC server
                // Just return Ok here
                Response::Ok
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};

    fn create_test_handler() -> DaemonCommandHandler {
        let config = Config::default();
        let transcriber: Arc<dyn crate::stt::transcriber::Transcriber> =
            Arc::new(WhisperTranscriber::new(WhisperConfig::default()).unwrap());

        #[cfg(feature = "portal")]
        let state = DaemonState::new(config, transcriber, None);

        #[cfg(not(feature = "portal"))]
        let state = DaemonState::new(config, transcriber);

        DaemonCommandHandler::new(state, true, 0)
    }

    #[tokio::test]
    async fn test_handler_status() {
        let handler = create_test_handler();
        let response = handler.handle(Command::Status).await;

        match response {
            Response::Status {
                recording,
                model_loaded,
                model_name,
            } => {
                assert!(!recording, "Should not be recording initially");
                assert!(model_loaded, "Model should be loaded");
                assert!(model_name.is_some(), "Model name should be present");
            }
            _ => panic!("Expected Status response"),
        }
    }

    #[tokio::test]
    async fn test_handler_stop_when_not_recording() {
        let handler = create_test_handler();
        let response = handler.handle(Command::Stop).await;

        match response {
            Response::Error { message } => {
                assert_eq!(message, "Not recording");
            }
            _ => panic!("Expected Error response when not recording"),
        }
    }

    #[tokio::test]
    async fn test_handler_cancel_when_not_recording() {
        let handler = create_test_handler();
        let response = handler.handle(Command::Cancel).await;

        match response {
            Response::Error { message } => {
                assert_eq!(message, "Not recording");
            }
            _ => panic!("Expected Error response when not recording"),
        }
    }

    #[tokio::test]
    async fn test_handler_shutdown() {
        let handler = create_test_handler();
        let response = handler.handle(Command::Shutdown).await;

        assert_eq!(response, Response::Ok);
    }

    #[tokio::test]
    async fn test_get_status() {
        let handler = create_test_handler();
        let response = handler.get_status().await;

        match response {
            Response::Status {
                recording,
                model_loaded,
                model_name,
            } => {
                assert!(!recording);
                assert!(model_loaded);
                assert!(model_name.is_some());
            }
            _ => panic!("Expected Status response"),
        }
    }

    #[tokio::test]
    async fn test_cancel_recording() {
        let handler = create_test_handler();
        // Try to cancel when not recording
        let response = handler.cancel_recording().await;

        match response {
            Response::Error { message } => {
                assert_eq!(message, "Not recording");
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_state_not_recording_initially() {
        let handler = create_test_handler();
        assert!(!handler.state.is_recording().await);
    }

    #[tokio::test]
    async fn test_handler_implements_command_handler_trait() {
        let handler = create_test_handler();

        // Verify all command variants can be handled
        let _ = handler.handle(Command::Status).await;
        let _ = handler.handle(Command::Start).await;
        let _ = handler.handle(Command::Stop).await;
        let _ = handler.handle(Command::Cancel).await;
        let _ = handler.handle(Command::Toggle).await;
        let _ = handler.handle(Command::Shutdown).await;
    }
}
