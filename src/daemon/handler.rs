//! Command handler implementation for the daemon.

use crate::audio::capture::CpalAudioSource;
use crate::audio::recorder::AudioSource;
use crate::audio::vad::VadConfig;
use crate::config::{Config, resolve_hallucination_filters};
use crate::daemon::DaemonState;
use crate::inject::focused_window::reset_detection_cache;
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
        #[cfg(feature = "portal")]
        let sink = self.create_sink(&config, self.state.portal.clone());

        #[cfg(not(feature = "portal"))]
        let sink = self.create_sink(&config);

        // Build post-processors
        let post_processors = build_post_processors(&config);

        // Reset window detection cache so a fresh session re-probes compositors
        reset_detection_cache();

        // Start pipeline
        let transcriber = self.state.transcriber.read().await.clone();
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
            allowed_languages: self.state.allowed_languages.clone(),
            min_confidence: self.state.min_confidence.clone(),
            ..Default::default()
        }
    }

    /// Create sink with portal support based on config.
    #[cfg(feature = "portal")]
    fn create_sink(
        &self,
        config: &Config,
        portal: Option<Arc<crate::inject::portal::PortalSession>>,
    ) -> Box<dyn crate::pipeline::sink::TextSink> {
        Box::new(InjectorSink::with_portal(
            config.injection.method.clone(),
            config.injection.paste_key.clone(),
            self.verbosity,
            portal,
            config.injection.backend.clone(),
        ))
    }

    /// Create sink without portal support.
    #[cfg(not(feature = "portal"))]
    fn create_sink(&self, config: &Config) -> Box<dyn crate::pipeline::sink::TextSink> {
        Box::new(InjectorSink::system(
            config.injection.method.clone(),
            config.injection.paste_key.clone(),
            self.verbosity,
            config.injection.backend.clone(),
        ))
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
                self.state.emit(DaemonEvent::Transcription {
                    text: text.clone(),
                    language: String::new(),
                    confidence: 1.0,
                    wait_ms: None,
                    token_probabilities: vec![],
                });
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

    /// Handle set language command.
    async fn handle_set_language(&self, language: String) -> Response {
        // Validate language
        use crate::pipeline::post_processor::SUPPORTED_LANGUAGES;
        if language != "auto" && !SUPPORTED_LANGUAGES.contains(&language.as_str()) {
            return Response::Error {
                message: format!(
                    "Unsupported language '{}'. Supported: auto, {}",
                    language,
                    SUPPORTED_LANGUAGES.join(", ")
                ),
            };
        }

        // Compute new allowed_languages list
        let new_langs = if language == "auto" {
            Vec::new()
        } else {
            vec![language.clone()]
        };

        // Update config (for persistence and next recording)
        let mut config = self.state.config.lock().await;
        config.stt.language = language.clone();
        config.stt.allowed_languages = new_langs.clone();
        drop(config);

        // Update shared Arc (for live pipeline if recording)
        // RwLock poisoning is rare and unrecoverable; if it happens we want to fail loudly.
        #[allow(clippy::expect_used)]
        {
            *self
                .state
                .allowed_languages
                .write()
                .expect("allowed_languages RwLock poisoned") = new_langs;
        }

        // Emit event
        self.state.emit(DaemonEvent::ConfigChanged {
            key: "language".to_string(),
            value: language,
        });

        Response::Ok {
            message: "Language updated".to_string(),
        }
    }

    /// Handle list languages command.
    async fn handle_list_languages(&self) -> Response {
        use crate::pipeline::post_processor::SUPPORTED_LANGUAGES;

        let current = self.state.language().await;
        let mut languages = vec!["auto".to_string()];
        languages.extend(SUPPORTED_LANGUAGES.iter().map(|s| s.to_string()));

        Response::Languages { languages, current }
    }

    /// Handle set model command.
    #[cfg(feature = "whisper")]
    async fn handle_set_model(&self, model: String) -> Response {
        use crate::models::catalog::get_model;
        use crate::models::download::{download_model, is_model_installed};

        // Validate model exists in catalog
        if get_model(&model).is_none() {
            return Response::Error {
                message: format!("Unknown model '{}'", model),
            };
        }

        // Emit checking event
        self.state.emit(DaemonEvent::ModelLoading {
            model: model.clone(),
            progress: "checking".to_string(),
        });

        // Check if installed, download if needed
        if !is_model_installed(&model) {
            self.state.emit(DaemonEvent::ModelLoading {
                model: model.clone(),
                progress: "downloading".to_string(),
            });

            if let Err(e) = download_model(&model, true).await {
                let error_msg = format!("Download failed: {}", e);
                self.state.emit(DaemonEvent::ModelLoadFailed {
                    model: model.clone(),
                    error: error_msg.clone(),
                });
                return Response::Error { message: error_msg };
            }
        }

        // Emit loading event
        self.state.emit(DaemonEvent::ModelLoading {
            model: model.clone(),
            progress: "loading".to_string(),
        });

        // Create new transcriber with modified config
        let mut new_config = self.state.config.lock().await.clone();
        new_config.stt.model = model.clone();

        match crate::daemon::create_transcriber(&new_config, true, self.verbosity, false).await {
            Ok(new_transcriber) => {
                // Swap transcriber — safe during recording because the pipeline
                // holds its own Arc clone (taken at start_recording). The old model
                // stays alive via refcount until the pipeline finishes.
                *self.state.transcriber.write().await = new_transcriber;

                // Update config
                self.state.config.lock().await.stt.model = model.clone();

                // Emit success events
                self.state.emit(DaemonEvent::ModelLoaded {
                    model: model.clone(),
                });
                self.state.emit(DaemonEvent::ConfigChanged {
                    key: "model".to_string(),
                    value: model,
                });

                Response::Ok {
                    message: "Model loaded".to_string(),
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to load model: {}", e);
                self.state.emit(DaemonEvent::ModelLoadFailed {
                    model: model.clone(),
                    error: error_msg.clone(),
                });
                Response::Error { message: error_msg }
            }
        }
    }

    /// Handle set model command (when whisper feature is disabled).
    #[cfg(not(feature = "whisper"))]
    async fn handle_set_model(&self, _model: String) -> Response {
        Response::Error {
            message: "Model switching requires the whisper feature".to_string(),
        }
    }

    /// Handle list models command.
    async fn handle_list_models(&self) -> Response {
        use crate::ipc::protocol::ModelInfoResponse;
        use crate::models::catalog::list_models;
        use crate::models::download::is_model_installed;

        let current = self.state.model_name().await;
        let catalog = list_models();

        let models: Vec<ModelInfoResponse> = catalog
            .iter()
            .map(|model_info| ModelInfoResponse {
                name: model_info.name.to_string(),
                size_mb: model_info.size_mb,
                english_only: model_info.english_only,
                installed: is_model_installed(model_info.name),
                quantized: model_info.quantized,
            })
            .collect();

        Response::Models { models, current }
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
            Command::SetLanguage { language } => self.handle_set_language(language).await,
            Command::ListLanguages => self.handle_list_languages().await,
            Command::SetModel { model } => self.handle_set_model(model).await,
            Command::ListModels => self.handle_list_models().await,
        }
    }

    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<DaemonEvent>> {
        Some(self.state.subscribe())
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
        let state = DaemonState::new(
            config,
            transcriber,
            #[cfg(feature = "portal")]
            None,
        );
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

    #[tokio::test]
    async fn test_handler_follow_command_returns_error() {
        let handler = create_test_handler();
        let response = handler.handle(Command::Follow).await;

        match response {
            Response::Error { message } => {
                assert_eq!(
                    message, "Follow command not supported via request-response handler",
                    "Follow should return specific error message"
                );
            }
            _ => panic!("Expected Error response for Follow command"),
        }
    }

    #[tokio::test]
    async fn test_handler_subscribe() {
        let handler = create_test_handler();

        // Subscribe to events
        let mut rx = handler.subscribe();

        // Emit a test event
        handler.state.emit(DaemonEvent::Transcription {
            text: "test".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
            token_probabilities: vec![],
        });

        // Should receive the event
        let event = rx.recv().await.expect("Should receive event");
        match event {
            DaemonEvent::Transcription {
                text,
                language,
                confidence,
                ..
            } => {
                assert_eq!(text, "test");
                assert_eq!(language, "en");
                assert_eq!(confidence, 0.95);
            }
            _ => panic!("Expected Transcription event"),
        }
    }

    #[tokio::test]
    async fn test_toggle_recording_when_not_recording() {
        let handler = create_test_handler();

        // Ensure not recording
        assert!(!handler.state.is_recording().await);

        // Toggle should try to start
        let response = handler.toggle_recording().await;

        // Will either succeed or fail (audio device unavailable in test)
        match response {
            Response::Ok { message } => {
                assert_eq!(message, "Recording started");
            }
            Response::Error { .. } => {
                // Audio device might not be available in test env
            }
            _ => panic!("Expected Ok or Error"),
        }
    }

    #[tokio::test]
    async fn test_stop_recording_when_not_recording() {
        let handler = create_test_handler();

        let response = handler.stop_recording().await;

        match response {
            Response::Error { message } => {
                assert_eq!(
                    message, "Not recording",
                    "Stop should return 'Not recording' error"
                );
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_build_pipeline_config() {
        let handler = create_test_handler();
        let config = handler.state.config.lock().await.clone();

        let pipeline_config = handler.build_pipeline_config(&config);

        // Verify configuration fields
        assert_eq!(
            pipeline_config.sample_rate, 16000,
            "Sample rate should be 16kHz for Whisper"
        );
        assert_eq!(
            pipeline_config.vad.speech_threshold, config.audio.vad_threshold,
            "VAD threshold should match config"
        );
        assert_eq!(
            pipeline_config.vad.silence_duration_ms, config.audio.silence_duration_ms,
            "Silence duration should match config"
        );
        assert_eq!(
            pipeline_config.verbosity, 0,
            "Verbosity should match handler verbosity"
        );
        assert!(pipeline_config.auto_level, "Auto level should be enabled");
        assert!(
            pipeline_config.quiet,
            "Quiet should be true in test handler"
        );
        assert!(pipeline_config.event_tx.is_some(), "Event TX should be set");
        assert_eq!(
            *pipeline_config.allowed_languages.read().unwrap(),
            config.stt.allowed_languages,
            "Allowed languages should match config"
        );
        assert_eq!(
            *pipeline_config.min_confidence.read().unwrap(),
            config.stt.min_confidence,
            "Min confidence should match config"
        );
    }

    #[tokio::test]
    async fn test_handler_new_with_different_verbosity() {
        let config = Config::default();
        let transcriber: Arc<dyn crate::stt::transcriber::Transcriber> =
            Arc::new(MockTranscriber::new("mock-test-model"));
        let state = DaemonState::new(
            config,
            transcriber,
            #[cfg(feature = "portal")]
            None,
        );
        let handler = DaemonCommandHandler::new(state, false, 2);

        assert_eq!(handler.verbosity, 2, "Verbosity should be set correctly");
        assert!(!handler.quiet, "Quiet should be false");
    }

    #[tokio::test]
    async fn test_handler_emits_events_on_state_changes() {
        let handler = create_test_handler();
        let mut rx = handler.subscribe();

        // Try to start recording (will fail in test env, but should still emit events if successful)
        let response = handler.start_recording().await;

        // Check if we got a recording state changed event
        match response {
            Response::Ok { .. } => {
                // If recording started successfully, we should get an event
                let event = rx
                    .recv()
                    .await
                    .expect("Should receive recording state event");
                assert_eq!(
                    event,
                    DaemonEvent::RecordingStateChanged { recording: true },
                    "Should emit recording started event"
                );
            }
            Response::Error { .. } => {
                // Audio device not available, no event emitted
            }
            _ => panic!("Unexpected response type"),
        }
    }

    #[tokio::test]
    async fn test_handler_status_shows_correct_backend() {
        let handler = create_test_handler();

        let response = handler.get_status().await;

        match response {
            Response::Status { backend, .. } => {
                assert!(
                    !backend.is_empty(),
                    "Backend should be initialized to a non-empty value"
                );
            }
            _ => panic!("Expected Status response"),
        }
    }

    #[tokio::test]
    async fn test_create_audio_source_with_invalid_device() {
        let handler = create_test_handler();
        let mut config = handler.state.config.lock().await.clone();

        // Set an invalid device name
        config.audio.device = Some("nonexistent-audio-device-12345".to_string());

        let result = handler.create_audio_source(&config);

        // Should return an error
        match result {
            Err(Response::Error { message }) => {
                assert!(
                    message.contains("Failed to create audio source"),
                    "Error message should mention audio source failure"
                );
                assert!(
                    message.contains("nonexistent-audio-device-12345"),
                    "Error message should include device name"
                );
            }
            Ok(_) => {
                // Might succeed if the audio backend is very permissive
            }
            Err(_) => panic!("Expected Response::Error"),
        }
    }

    #[tokio::test]
    async fn test_create_sink() {
        let handler = create_test_handler();
        let config = handler.state.config.lock().await.clone();

        // create_sink should not panic
        #[cfg(feature = "portal")]
        let _sink = handler.create_sink(&config, None);

        #[cfg(not(feature = "portal"))]
        let _sink = handler.create_sink(&config);

        // Test passes if we didn't panic
    }

    // New command handler tests

    #[tokio::test]
    async fn test_handle_set_language_valid() {
        let handler = create_test_handler();
        let response = handler.handle_set_language("de".to_string()).await;

        match response {
            Response::Ok { message } => {
                assert_eq!(message, "Language updated");
            }
            _ => panic!("Expected Ok response"),
        }

        // Verify config was updated
        let language = handler.state.language().await;
        assert_eq!(language, "de");

        // Verify allowed_languages enforces the selected language
        let config = handler.state.config.lock().await;
        assert_eq!(config.stt.allowed_languages, vec!["de"]);
    }

    #[tokio::test]
    async fn test_handle_set_language_auto() {
        let handler = create_test_handler();
        // First set a specific language
        handler.handle_set_language("de".to_string()).await;
        // Then switch to auto
        let response = handler.handle_set_language("auto".to_string()).await;

        assert!(
            matches!(response, Response::Ok { .. }),
            "auto should be valid"
        );

        // Verify allowed_languages is cleared (accept all)
        let config = handler.state.config.lock().await;
        assert!(
            config.stt.allowed_languages.is_empty(),
            "auto should clear allowed_languages"
        );
    }

    #[tokio::test]
    async fn test_handle_set_language_invalid() {
        let handler = create_test_handler();
        let response = handler.handle_set_language("invalid".to_string()).await;

        match response {
            Response::Error { message } => {
                assert!(message.contains("Unsupported language"));
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[tokio::test]
    async fn test_handle_set_language_emits_config_changed() {
        let handler = create_test_handler();
        let mut rx = handler.state.subscribe();

        let response = handler.handle_set_language("de".to_string()).await;
        assert!(matches!(response, Response::Ok { .. }));

        let event = rx.recv().await.expect("Should receive event");
        assert_eq!(
            event,
            DaemonEvent::ConfigChanged {
                key: "language".to_string(),
                value: "de".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_handle_list_languages() {
        let handler = create_test_handler();
        let response = handler.handle_list_languages().await;

        match response {
            Response::Languages { languages, current } => {
                assert!(languages.contains(&"auto".to_string()));
                assert!(languages.contains(&"en".to_string()));
                assert!(languages.contains(&"de".to_string()));
                assert_eq!(current, "auto");
            }
            _ => panic!("Expected Languages response"),
        }
    }

    #[tokio::test]
    async fn test_handle_list_models() {
        let handler = create_test_handler();
        let response = handler.handle_list_models().await;

        match response {
            Response::Models { models, current } => {
                assert!(!models.is_empty(), "Should have at least one model");
                assert_eq!(current, "base");
                // Verify structure
                for model in models {
                    assert!(!model.name.is_empty());
                    assert!(model.size_mb > 0);
                }
            }
            _ => panic!("Expected Models response"),
        }
    }

    #[tokio::test]
    async fn test_handle_set_model_without_whisper_feature() {
        let handler = create_test_handler();

        // Without whisper feature, model switching returns a clear error
        #[cfg(not(feature = "whisper"))]
        {
            let response = handler.handle_set_model("base".to_string()).await;
            match response {
                Response::Error { message } => {
                    assert!(message.contains("whisper feature"));
                }
                _ => panic!("Expected error when whisper feature disabled"),
            }
        }

        // With whisper feature, we can't test actual loading without model files
        #[cfg(feature = "whisper")]
        {
            let _ = handler;
        }
    }

    #[tokio::test]
    async fn test_subscribe_returns_some() {
        let handler = create_test_handler();

        // The trait method subscribe() should return Some
        let receiver = CommandHandler::subscribe(&handler);
        assert!(receiver.is_some(), "subscribe() should return Some");
    }

    #[tokio::test]
    async fn test_subscribe_receives_events() {
        let handler = create_test_handler();

        let mut rx = CommandHandler::subscribe(&handler).expect("Should return Some");

        // Emit event
        handler.state.emit(DaemonEvent::Transcription {
            text: "test".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
            token_probabilities: vec![],
        });

        // Should receive
        let event = rx.recv().await.expect("Should receive event");
        match event {
            DaemonEvent::Transcription {
                text,
                language,
                confidence,
                ..
            } => {
                assert_eq!(text, "test");
                assert_eq!(language, "en");
                assert_eq!(confidence, 0.95);
            }
            _ => panic!("Expected Transcription event"),
        }
    }

    #[tokio::test]
    async fn test_new_commands_in_trait_impl() {
        let handler = create_test_handler();

        // Test SetLanguage command
        let response = handler
            .handle(Command::SetLanguage {
                language: "en".to_string(),
            })
            .await;
        assert!(
            matches!(response, Response::Ok { .. }),
            "SetLanguage should return Ok"
        );

        // Test ListLanguages command
        let response = handler.handle(Command::ListLanguages).await;
        assert!(
            matches!(response, Response::Languages { .. }),
            "ListLanguages should return Languages"
        );

        // Test ListModels command
        let response = handler.handle(Command::ListModels).await;
        assert!(
            matches!(response, Response::Models { .. }),
            "ListModels should return Models"
        );
    }
}
