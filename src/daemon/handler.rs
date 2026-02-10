//! Command handler implementation for the daemon.

use crate::audio::capture::CpalAudioSource;
use crate::audio::recorder::AudioSource;
use crate::audio::vad::VadConfig;
use crate::config::{Config, resolve_hallucination_filters};
use crate::daemon::DaemonState;
use crate::input::focused_window::reset_detection_cache;
use crate::ipc::protocol::{Command, DaemonEvent, Response};
use crate::ipc::server::CommandHandler;
use crate::pipeline::adaptive_chunker::AdaptiveChunkerConfig;
use crate::pipeline::orchestrator::{Pipeline, PipelineConfig};
use crate::pipeline::post_processor::build_post_processors;
use crate::pipeline::sink::InjectorSink;
use std::sync::Arc;

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

    /// Subscribe to daemon events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaemonEvent> {
        self.state.subscribe()
    }

    /// Start recording.
    async fn start_recording(&self) -> Response {
        // Lock pipeline for entire operation to prevent race conditions
        let mut pipeline_guard = self.state.pipeline.lock().await;

        // Check if already recording
        if pipeline_guard.is_some() {
            return Response::Error {
                message: "Already recording".to_string(),
            };
        }

        // Get config
        let config = self.state.config.lock().await.clone();

        // Create audio source
        let audio_source = match self.create_audio_source(&config) {
            Ok(source) => source,
            Err(e) => return e,
        };

        // Build pipeline configuration
        let pipeline_config = self.build_pipeline_config(&config);

        // Create sink
        let sink = self.create_sink(&config);

        // Build post-processors
        let post_processors = build_post_processors(&config);

        // Reset window detection cache so a fresh session re-probes compositors
        reset_detection_cache();

        // Start pipeline
        let transcriber = Arc::clone(&self.state.transcriber);
        let pipeline = Pipeline::new(pipeline_config);

        match pipeline.start_with_post_processors(audio_source, transcriber, sink, post_processors)
        {
            Ok(handle) => {
                *pipeline_guard = Some(handle);
                self.state
                    .emit(DaemonEvent::RecordingStateChanged { recording: true });
                Response::Ok {
                    message: "Recording started".to_string(),
                }
            }
            Err(e) => Response::Error {
                message: format!("Failed to start pipeline: {}", e),
            },
        }
    }

    /// Create audio source from config.
    fn create_audio_source(&self, config: &Config) -> Result<Box<dyn AudioSource>, Response> {
        let device_name = config.audio.device.as_deref();
        match CpalAudioSource::new(device_name) {
            Ok(source) => Ok(Box::new(source)),
            Err(e) => {
                let device_info = device_name.unwrap_or("default");
                Err(Response::Error {
                    message: format!(
                        "Failed to create audio source for device '{}': {}",
                        device_info, e
                    ),
                })
            }
        }
    }

    /// Build pipeline configuration from config.
    fn build_pipeline_config(&self, config: &Config) -> PipelineConfig {
        // Whisper models require 16kHz sample rate
        const WHISPER_SAMPLE_RATE: u32 = 16000;

        let hallucination_filters =
            resolve_hallucination_filters(&config.transcription.hallucination_filters);
        PipelineConfig {
            vad: VadConfig {
                speech_threshold: config.audio.vad_threshold,
                silence_duration_ms: config.audio.silence_duration_ms,
                ..Default::default()
            },
            chunker: AdaptiveChunkerConfig::default(),
            verbosity: self.verbosity,
            auto_level: true,
            quiet: self.quiet,
            sample_rate: WHISPER_SAMPLE_RATE,
            hallucination_filters,
            event_tx: Some(self.state.pipeline_event_tx.clone()),
            ..Default::default()
        }
    }

    /// Create sink with portal support based on config.
    fn create_sink(&self, config: &Config) -> Box<dyn crate::pipeline::sink::TextSink> {
        #[cfg(feature = "portal")]
        {
            Box::new(InjectorSink::with_portal(
                config.input.method.clone(),
                config.input.paste_key.clone(),
                self.verbosity,
                self.state.portal.clone(),
            ))
        }

        #[cfg(not(feature = "portal"))]
        {
            Box::new(InjectorSink::system(
                config.input.method.clone(),
                config.input.paste_key.clone(),
                self.verbosity,
            ))
        }
    }

    /// Stop recording and return transcription.
    async fn stop_recording(&self) -> Response {
        // Check if recording
        let mut pipeline_guard = self.state.pipeline.lock().await;

        if let Some(handle) = pipeline_guard.take() {
            // Stop pipeline and get result
            let result = handle.stop();

            self.state
                .emit(DaemonEvent::RecordingStateChanged { recording: false });
            if let Some(text) = result {
                self.state
                    .emit(DaemonEvent::Transcription { text: text.clone() });
                Response::Transcription { text }
            } else {
                Response::Ok {
                    message: "Recording stopped (no speech detected)".to_string(),
                }
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
            self.state
                .emit(DaemonEvent::RecordingStateChanged { recording: false });
            Response::Ok {
                message: "Recording cancelled".to_string(),
            }
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
        let language = Some(self.state.language().await);

        Response::Status {
            recording,
            model_loaded: true, // Model is always loaded in daemon
            model_name,
            language,
            daemon_version: crate::version_string(),
            backend: self.state.backend.clone(),
            device: self.state.device.clone(),
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
                Response::Ok {
                    message: "Daemon shutdown initiated".to_string(),
                }
            }
            Command::Follow => {
                // Follow is handled separately via streaming, not request-response
                Response::Error {
                    message: "Follow command not supported via request-response handler"
                        .to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::stt::transcriber::MockTranscriber;

    fn create_test_handler() -> DaemonCommandHandler {
        let config = Config::default();
        let transcriber: Arc<dyn crate::stt::transcriber::Transcriber> =
            Arc::new(MockTranscriber::new("mock-test-model"));

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
                language,
                daemon_version,
                backend,
                device,
            } => {
                assert!(!recording, "Should not be recording initially");
                assert!(model_loaded, "Model should be loaded");
                assert_eq!(
                    model_name,
                    Some("base".to_string()),
                    "Model name should be 'base' from default config"
                );
                assert_eq!(
                    language,
                    Some("auto".to_string()),
                    "Language should be 'auto' from default config"
                );
                assert!(!daemon_version.is_empty(), "Version should not be empty");
                assert!(!backend.is_empty(), "Backend should not be empty");
                // device may be None in test environment
                let _ = device;
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

        assert_eq!(
            response,
            Response::Ok {
                message: "Daemon shutdown initiated".to_string()
            }
        );
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
                language,
                daemon_version,
                backend,
                device,
            } => {
                assert!(!recording);
                assert!(model_loaded);
                assert_eq!(
                    model_name,
                    Some("base".to_string()),
                    "Model name should be 'base' from default config"
                );
                assert_eq!(
                    language,
                    Some("auto".to_string()),
                    "Language should be 'auto' from default config"
                );
                assert!(!daemon_version.is_empty(), "Version should not be empty");
                assert!(!backend.is_empty(), "Backend should not be empty");
                // device may be None in test environment
                let _ = device;
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

        // Test Status command
        let response = handler.handle(Command::Status).await;
        match response {
            Response::Status {
                recording,
                model_loaded,
                model_name,
                language,
                daemon_version,
                backend,
                device,
            } => {
                assert!(!recording, "Should not be recording initially");
                assert!(model_loaded, "Model should be loaded");
                assert!(model_name.is_some(), "Model name should be present");
                assert!(language.is_some(), "Language should be present");
                assert!(!daemon_version.is_empty(), "Version should not be empty");
                assert!(!backend.is_empty(), "Backend should not be empty");
                let _ = device;
            }
            _ => panic!("Expected Status response, got: {:?}", response),
        }

        // Test Start command (may fail in test env due to no audio device)
        let response = handler.handle(Command::Start).await;
        match response {
            Response::Ok { .. } => {} // start_recording may fail in test env (no audio device)
            Response::Error { .. } => {} // audio device unavailable
            _ => panic!("Expected Ok or Error"),
        }

        // Test Stop command (should fail since we didn't actually start - audio source creation would fail in test)
        let response = handler.handle(Command::Stop).await;
        match response {
            Response::Error { message } => {
                assert_eq!(
                    message, "Not recording",
                    "Stop should return error when not recording"
                );
            }
            _ => {} // May succeed if audio source was created
        }

        // Test Cancel command (should fail since not recording)
        let response = handler.handle(Command::Cancel).await;
        match response {
            Response::Error { message } => {
                assert_eq!(
                    message, "Not recording",
                    "Cancel should return error when not recording"
                );
            }
            _ => {} // May succeed if recording was started
        }

        // Test Toggle command (should try to stop since might be recording)
        let response = handler.handle(Command::Toggle).await;
        assert!(
            matches!(
                response,
                Response::Ok { .. } | Response::Error { .. } | Response::Transcription { .. }
            ),
            "Toggle should return Ok, Error, or Transcription"
        );

        // Test Shutdown command
        let response = handler.handle(Command::Shutdown).await;
        assert_eq!(
            response,
            Response::Ok {
                message: "Daemon shutdown initiated".to_string()
            },
            "Shutdown should return Ok"
        );
    }
}
