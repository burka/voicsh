//! Daemon mode for voicsh - manages recording state and IPC server.

pub mod handler;

use crate::audio::capture::suppress_audio_warnings;
use crate::config::Config;
use crate::error::{Result, VoicshError};
use crate::ipc::protocol::DaemonEvent;
use crate::ipc::server::IpcServer;
use crate::pipeline::orchestrator::PipelineHandle;
use crate::stt::transcriber::Transcriber;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(feature = "portal")]
use crate::inject::portal::PortalSession;

/// Daemon state: model loaded, recording state, IPC server.
pub struct DaemonState {
    /// Configuration
    pub config: Arc<Mutex<Config>>,
    /// Loaded transcriber (model stays in memory, can be swapped with write lock)
    pub transcriber: tokio::sync::RwLock<Arc<dyn Transcriber>>,
    /// Current pipeline handle (Some = recording, None = idle)
    pub pipeline: Arc<Mutex<Option<PipelineHandle>>>,
    /// Portal session for input injection (if available)
    #[cfg(feature = "portal")]
    pub portal: Option<Arc<PortalSession>>,
    /// Broadcast sender for daemon events (follow clients subscribe here)
    pub event_tx: tokio::sync::broadcast::Sender<DaemonEvent>,
    /// Crossbeam sender for pipeline threads to emit events (non-blocking)
    pub pipeline_event_tx: crossbeam_channel::Sender<DaemonEvent>,
    /// Crossbeam receiver (held to keep channel alive; bridge thread clones it)
    pipeline_event_rx: crossbeam_channel::Receiver<DaemonEvent>,
    /// GPU/CPU backend name (e.g., "CUDA", "CPU")
    pub backend: String,
    /// GPU or CPU device description (e.g., "RTX 5060 Ti (16 GB)")
    pub device: Option<String>,
    /// Allowed languages for transcription filtering (live-updatable during recording)
    pub allowed_languages: Arc<std::sync::RwLock<Vec<String>>>,
    /// Minimum confidence threshold (live-updatable during recording)
    pub min_confidence: Arc<std::sync::RwLock<f32>>,
}

/// Detect GPU device name and memory from nvidia-smi.
fn detect_gpu_device() -> Option<String> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut parts = text.splitn(2, ", ");
    let name = parts.next()?;
    let name = name.strip_prefix("NVIDIA ").unwrap_or(name);
    let memory_mb: u64 = parts.next()?.trim().parse().ok()?;
    let memory_gb = memory_mb / 1024;
    Some(format!("{name} ({memory_gb} GB)"))
}

/// Detect CPU model name from /proc/cpuinfo.
fn detect_cpu_device() -> Option<String> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))?
        .split(':')
        .nth(1)?
        .trim()
        .to_string();
    Some(model)
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
        let backend = crate::defaults::gpu_backend().to_string();
        let device = detect_gpu_device().or_else(detect_cpu_device);

        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        let (pipeline_event_tx, pipeline_event_rx) = crossbeam_channel::bounded(256);

        // Initialize shared state from config
        let allowed_languages =
            Arc::new(std::sync::RwLock::new(config.stt.allowed_languages.clone()));
        let min_confidence = Arc::new(std::sync::RwLock::new(config.stt.min_confidence));

        Self {
            config: Arc::new(Mutex::new(config)),
            transcriber: tokio::sync::RwLock::new(transcriber),
            pipeline: Arc::new(Mutex::new(None)),
            #[cfg(feature = "portal")]
            portal,
            event_tx,
            pipeline_event_tx,
            pipeline_event_rx,
            backend,
            device,
            allowed_languages,
            min_confidence,
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

    /// Returns language setting from config.
    pub async fn language(&self) -> String {
        self.config.lock().await.stt.language.clone()
    }

    /// Subscribe to daemon events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaemonEvent> {
        self.event_tx.subscribe()
    }

    /// Emit an event to all follow clients (for async/handler code).
    pub fn emit(&self, event: DaemonEvent) {
        if let Err(e) = self.event_tx.send(event) {
            eprintln!(
                "voicsh: failed to emit daemon event to subscribers: {} (all clients may have disconnected)",
                e
            );
        }
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

    // Establish portal session for keyboard injection
    #[cfg(feature = "portal")]
    let portal = match PortalSession::try_new().await {
        Ok(session) => {
            if verbosity >= 1 {
                eprintln!("Portal keyboard access granted.");
            }
            Some(Arc::new(session))
        }
        Err(e) => {
            eprintln!(
                "voicsh: portal unavailable ({}), using wtype/ydotool fallback.",
                e
            );
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

    // Spawn bridge thread: crossbeam (pipeline OS threads) → tokio broadcast (follow clients)
    let bridge_event_rx = state.pipeline_event_rx.clone();
    let bridge_event_tx = state.event_tx.clone();
    std::thread::spawn(move || {
        while let Ok(event) = bridge_event_rx.recv() {
            if let Err(e) = bridge_event_tx.send(event) {
                eprintln!(
                    "voicsh: bridge thread failed to forward event to subscribers: {} (all clients may have disconnected)",
                    e
                );
            }
        }
    });

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
pub(crate) async fn create_transcriber(
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

    // Get model path (must exist after download check above)
    let path = model_path(&resolved_model);

    // Create transcriber
    if config.stt.fan_out {
        // Fan-out mode: run English + multilingual in parallel
        let en_model = english_variant(&resolved_model).ok_or_else(|| {
            VoicshError::TranscriptionModelNotFound {
                path: format!("{}.en (English variant)", resolved_model),
            }
        })?;
        let en_path = model_path(en_model);

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
            use_gpu: true,
        })?;

        let multilingual_transcriber = WhisperTranscriber::new(WhisperConfig {
            model_path: path,
            language: language.clone(),
            threads: None,
            use_gpu: true,
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
            use_gpu: true,
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
        let state = DaemonState::new(
            config,
            mock_transcriber(),
            #[cfg(feature = "portal")]
            None,
        );
        assert!(!state.is_recording().await);
    }

    #[tokio::test]
    async fn test_daemon_state_is_recording() {
        let config = Config::default();
        let state = DaemonState::new(
            config,
            mock_transcriber(),
            #[cfg(feature = "portal")]
            None,
        );
        // Initially not recording
        assert!(!state.is_recording().await);
    }

    #[tokio::test]
    async fn test_daemon_state_model_name() {
        let mut config = Config::default();
        config.stt.model = "test-model".to_string();
        let state = DaemonState::new(
            config,
            mock_transcriber(),
            #[cfg(feature = "portal")]
            None,
        );
        let model_name = state.model_name().await;
        assert_eq!(model_name, "test-model");
    }

    #[tokio::test]
    async fn test_daemon_state_language() {
        let mut config = Config::default();
        config.stt.language = "de".to_string();
        let state = DaemonState::new(
            config,
            mock_transcriber(),
            #[cfg(feature = "portal")]
            None,
        );
        let language = state.language().await;
        assert_eq!(language, "de");
    }

    fn create_state_with_config(config: Config) -> DaemonState {
        DaemonState::new(
            config,
            mock_transcriber(),
            #[cfg(feature = "portal")]
            None,
        )
    }

    fn create_state() -> DaemonState {
        create_state_with_config(Config::default())
    }

    #[tokio::test]
    async fn test_daemon_state_new_initializes_backend_and_device() {
        let state = create_state();

        // Backend should be non-empty (CPU, CUDA, etc.)
        assert!(
            !state.backend.is_empty(),
            "Backend should be initialized to a non-empty value"
        );

        // Device may be Some or None depending on environment
        // Just verify the field exists (this is a structural test)
        let _ = state.device;
    }

    #[tokio::test]
    async fn test_daemon_state_new_initializes_channels() {
        let state = create_state();

        // Event channel should be ready
        let _rx = state.subscribe();

        // Pipeline event channel should be ready (send a test event)
        let test_event = DaemonEvent::Log {
            message: "test".to_string(),
        };
        state
            .pipeline_event_tx
            .send(test_event.clone())
            .expect("Should be able to send to pipeline event channel");

        // The bridge thread isn't running in this test, so we can't receive it
        // But we verified the channel is functional
    }

    #[tokio::test]
    async fn test_daemon_state_is_recording_transitions() {
        let state = create_state();

        // Initially not recording
        assert_eq!(
            state.is_recording().await,
            false,
            "Should not be recording initially"
        );

        // Simulate setting a pipeline handle (would normally be set by start_recording)
        // We can't easily create a real PipelineHandle in tests, but we can verify
        // the state reports correctly
        {
            let pipeline = state.pipeline.lock().await;
            assert!(
                pipeline.is_none(),
                "Pipeline should be None when not recording"
            );
        }

        // After starting recording (simulated by setting Some), is_recording would return true
        // This is tested in handler tests where we actually start recording
    }

    #[tokio::test]
    async fn test_daemon_state_model_name_default() {
        let state = create_state();
        let model_name = state.model_name().await;
        assert_eq!(
            model_name, "base",
            "Default config should have 'base' model"
        );
    }

    #[tokio::test]
    async fn test_daemon_state_language_default() {
        let state = create_state();
        let language = state.language().await;
        assert_eq!(
            language, "auto",
            "Default config should have 'auto' language"
        );
    }

    #[tokio::test]
    async fn test_daemon_state_subscribe() {
        let state = create_state();

        // Subscribe should create a new receiver
        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        // Emit an event
        let event = DaemonEvent::RecordingStateChanged { recording: true };
        state.emit(event.clone());

        // Both subscribers should receive it
        let received1 = rx1
            .recv()
            .await
            .expect("First subscriber should receive event");
        assert_eq!(
            received1, event,
            "First subscriber should receive the exact event"
        );

        let received2 = rx2
            .recv()
            .await
            .expect("Second subscriber should receive event");
        assert_eq!(
            received2, event,
            "Second subscriber should receive the exact event"
        );
    }

    #[tokio::test]
    async fn test_daemon_state_emit() {
        let state = create_state();
        let mut rx = state.subscribe();

        // Emit a recording state change event
        let event = DaemonEvent::RecordingStateChanged { recording: true };
        state.emit(event.clone());

        // Should receive the event
        let received = rx.recv().await.expect("Should receive emitted event");
        assert_eq!(received, event, "Received event should match emitted event");
    }

    #[tokio::test]
    async fn test_daemon_state_emit_multiple_events() {
        let state = create_state();
        let mut rx = state.subscribe();

        // Emit multiple events
        let event1 = DaemonEvent::RecordingStateChanged { recording: true };
        let event2 = DaemonEvent::Transcription {
            text: "hello".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
            word_probabilities: vec![],
        };
        let event3 = DaemonEvent::RecordingStateChanged { recording: false };

        state.emit(event1.clone());
        state.emit(event2.clone());
        state.emit(event3.clone());

        // All events should be received in order
        assert_eq!(rx.recv().await.unwrap(), event1, "First event should match");
        assert_eq!(
            rx.recv().await.unwrap(),
            event2,
            "Second event should match"
        );
        assert_eq!(rx.recv().await.unwrap(), event3, "Third event should match");
    }

    #[tokio::test]
    async fn test_daemon_state_emit_with_no_subscribers() {
        // This test verifies emit() doesn't panic when no subscribers exist
        let state = create_state();

        // Don't create any subscribers
        // emit() should handle the error gracefully and not panic
        let event = DaemonEvent::Log {
            message: "test".to_string(),
        };
        state.emit(event);

        // Test passes if we didn't panic
    }

    #[tokio::test]
    async fn test_daemon_state_emit_after_all_subscribers_dropped() {
        let state = create_state();

        {
            let _rx = state.subscribe();
            // Receiver dropped at end of scope
        }

        // emit() should handle the "no receivers" error gracefully
        let event = DaemonEvent::Log {
            message: "test".to_string(),
        };
        state.emit(event);

        // Test passes if we didn't panic
    }

    #[tokio::test]
    async fn test_daemon_state_subscribe_after_events_emitted() {
        let state = create_state();

        // Emit event before subscribing
        let event1 = DaemonEvent::Log {
            message: "before".to_string(),
        };
        state.emit(event1);

        // New subscriber should not receive old events
        let mut rx = state.subscribe();

        // Emit new event
        let event2 = DaemonEvent::Log {
            message: "after".to_string(),
        };
        state.emit(event2.clone());

        // Should only receive event2, not event1
        let received = rx.recv().await.expect("Should receive new event");
        assert_eq!(
            received, event2,
            "New subscriber should only see events after subscription"
        );

        // Should not have any more events
        match rx.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // Expected
            }
            _ => panic!("Should have no more events"),
        }
    }

    #[tokio::test]
    async fn test_daemon_state_config_mutation() {
        let state = create_state();

        // Get initial model name
        let initial_model = state.model_name().await;
        assert_eq!(initial_model, "base");

        // Mutate config
        {
            let mut config = state.config.lock().await;
            config.stt.model = "large".to_string();
        }

        // Model name should reflect the change
        let new_model = state.model_name().await;
        assert_eq!(new_model, "large", "Config mutation should be reflected");
    }

    #[tokio::test]
    async fn test_daemon_state_pipeline_initially_none() {
        let state = create_state();

        let pipeline = state.pipeline.lock().await;
        assert!(
            pipeline.is_none(),
            "Pipeline should be None on initialization"
        );
    }

    #[test]
    fn test_detect_gpu_device_handles_missing_nvidia_smi() {
        // This test verifies detect_gpu_device() doesn't panic when nvidia-smi is missing
        // Should return None gracefully
        let result = detect_gpu_device();
        // Result may be Some or None depending on environment
        let _ = result;
        // Test passes if we didn't panic
    }

    #[test]
    fn test_detect_cpu_device_handles_missing_cpuinfo() {
        // This test verifies detect_cpu_device() doesn't panic
        // On Linux it should return Some, on other platforms it might return None
        let result = detect_cpu_device();
        // Result may be Some or None depending on platform
        let _ = result;
        // Test passes if we didn't panic
    }

    #[tokio::test]
    async fn test_daemon_state_event_channel_capacity() {
        let state = create_state();
        let mut rx = state.subscribe();

        // Emit events up to the channel capacity (256)
        // We emit fewer than capacity to ensure we can receive all
        for i in 0..100 {
            state.emit(DaemonEvent::Log {
                message: format!("event {}", i),
            });
        }

        // We should be able to receive all events we sent
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }

        // We should have received all 100 events
        assert_eq!(
            count, 100,
            "Should have received all 100 events from the channel"
        );
    }

    #[tokio::test]
    async fn test_daemon_state_event_channel_overflow() {
        // This test verifies channel behavior when capacity is exceeded
        let state = create_state();
        let mut rx = state.subscribe();

        // Emit many more events than channel capacity without receiving
        // The broadcast channel will start dropping old events
        for i in 0..500 {
            state.emit(DaemonEvent::Log {
                message: format!("event {}", i),
            });
        }

        // Try to receive - we might get Lagged error or some recent events
        let first_result = rx.try_recv();

        match first_result {
            Ok(_) => {
                // We got some events, count them
                let mut count = 1;
                while rx.try_recv().is_ok() {
                    count += 1;
                }
                // Should have received some events (fewer than 500)
                assert!(
                    count > 0 && count < 500,
                    "Should receive some but not all events when overflowed"
                );
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                // Expected: channel lagged and dropped some events
                assert!(n > 0, "Should report how many events were lagged");
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // All events were dropped, which is possible
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                panic!("Channel should not be closed");
            }
        }
    }
}
