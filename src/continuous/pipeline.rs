//! Continuous audio pipeline that runs from startup until shutdown.

use crate::audio::capture::CpalAudioSource;
use crate::audio::recorder::AudioSource;
use crate::audio::vad::VadConfig;
use crate::config::InputMethod;
use crate::continuous::adaptive_chunker::AdaptiveChunkerConfig;
use crate::continuous::error::{ErrorReporter, LogReporter};
use crate::continuous::station::StationRunner;
use crate::continuous::types::AudioFrame;
use crate::continuous::{ChunkerStation, InjectorStation, TranscriberStation, VadStation};
use crate::error::Result;
use crate::stt::transcriber::Transcriber;
use crossbeam_channel::bounded;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Configuration for the continuous pipeline.
#[derive(Debug, Clone)]
pub struct ContinuousPipelineConfig {
    /// VAD configuration
    pub vad: VadConfig,
    /// Chunker configuration
    pub chunker: AdaptiveChunkerConfig,
    /// Show audio level meter
    pub show_levels: bool,
    /// Enable auto-leveling for VAD
    pub auto_level: bool,
    /// Suppress output messages
    pub quiet: bool,
    /// Text injection method
    pub input_method: InputMethod,
    /// Paste key: "auto" for auto-detection, or explicit like "ctrl+v"
    pub paste_key: String,
    /// Sample rate
    pub sample_rate: u32,
    /// Channel buffer sizes
    pub audio_buffer: usize,
    pub vad_buffer: usize,
    pub chunk_buffer: usize,
    pub transcribe_buffer: usize,
}

impl Default for ContinuousPipelineConfig {
    fn default() -> Self {
        Self {
            vad: VadConfig::default(),
            chunker: AdaptiveChunkerConfig::default(),
            show_levels: false,
            auto_level: true,
            quiet: false,
            input_method: InputMethod::Clipboard,
            paste_key: "auto".to_string(),
            sample_rate: 16000,
            audio_buffer: 32,
            vad_buffer: 16,
            chunk_buffer: 4,
            transcribe_buffer: 4,
        }
    }
}

/// Handle to a running continuous pipeline.
pub struct ContinuousPipelineHandle {
    /// Flag to signal shutdown
    running: Arc<AtomicBool>,
    /// Join handles for spawned threads
    threads: Vec<JoinHandle<()>>,
}

impl ContinuousPipelineHandle {
    /// Stops the pipeline gracefully.
    pub fn stop(mut self) {
        // Signal shutdown
        self.running.store(false, Ordering::SeqCst);

        // Wait for all threads to complete
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }

    /// Returns true if the pipeline is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Continuous audio pipeline.
pub struct ContinuousPipeline {
    config: ContinuousPipelineConfig,
    error_reporter: Arc<dyn ErrorReporter>,
}

impl ContinuousPipeline {
    /// Creates a new continuous pipeline with default error reporter.
    pub fn new(config: ContinuousPipelineConfig) -> Self {
        Self {
            config,
            error_reporter: Arc::new(LogReporter),
        }
    }

    /// Sets a custom error reporter.
    pub fn with_error_reporter(mut self, reporter: Arc<dyn ErrorReporter>) -> Self {
        self.error_reporter = reporter;
        self
    }

    /// Starts the continuous pipeline.
    ///
    /// # Arguments
    /// * `audio_source` - Audio capture source
    /// * `transcriber` - Transcriber for speech-to-text
    ///
    /// # Returns
    /// Handle to control and stop the pipeline
    pub fn start<T: Transcriber + Send + Sync + 'static>(
        self,
        mut audio_source: CpalAudioSource,
        transcriber: Arc<T>,
    ) -> Result<ContinuousPipelineHandle> {
        let running = Arc::new(AtomicBool::new(true));
        let sequence = Arc::new(AtomicU64::new(0));

        // Create channels between stations
        let (audio_tx, audio_rx) = bounded(self.config.audio_buffer);
        let (vad_tx, vad_rx) = bounded(self.config.vad_buffer);
        let (chunk_tx, chunk_rx) = bounded(self.config.chunk_buffer);
        let (transcribe_tx, transcribe_rx) = bounded(self.config.transcribe_buffer);

        // Create stations
        let vad_station = VadStation::new(self.config.vad)
            .with_show_levels(self.config.show_levels)
            .with_auto_level(self.config.auto_level)
            .with_sample_rate(self.config.sample_rate);

        let chunker_station = ChunkerStation::new(self.config.chunker)
            .with_sample_rate(self.config.sample_rate)
            .with_quiet(self.config.quiet);

        let transcriber_station =
            TranscriberStation::new(transcriber).with_quiet(self.config.quiet);

        let injector_station =
            InjectorStation::new(self.config.input_method, self.config.paste_key)
                .with_quiet(self.config.quiet);

        // Spawn station runners
        let vad_runner =
            StationRunner::spawn(vad_station, audio_rx, vad_tx, self.error_reporter.clone());

        let chunker_runner = StationRunner::spawn(
            chunker_station,
            vad_rx,
            chunk_tx,
            self.error_reporter.clone(),
        );

        let transcriber_runner = StationRunner::spawn(
            transcriber_station,
            chunk_rx,
            transcribe_tx,
            self.error_reporter.clone(),
        );

        // For the terminal station, create a dummy output channel
        let (inject_tx, inject_rx) = bounded::<()>(self.config.transcribe_buffer);

        let injector_runner = StationRunner::spawn(
            injector_station,
            transcribe_rx,
            inject_tx,
            self.error_reporter.clone(),
        );

        // Drain the inject_rx in a separate thread
        let drain_running = running.clone();
        let drain_handle = thread::spawn(move || {
            while drain_running.load(Ordering::SeqCst) {
                if inject_rx.recv_timeout(Duration::from_millis(100)).is_err() {
                    // Timeout or disconnected - check if we should exit
                    if !drain_running.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        });

        // Start audio capture
        audio_source.start()?;

        // Spawn audio polling thread
        let audio_running = running.clone();
        let audio_sequence = sequence.clone();
        let audio_handle = thread::spawn(move || {
            // Poll audio source at ~60Hz (every 16ms)
            let poll_interval = Duration::from_millis(16);

            while audio_running.load(Ordering::SeqCst) {
                // Read samples from audio source
                let samples = match audio_source.read_samples() {
                    Ok(s) => s,
                    Err(_) => {
                        // Error reading samples - continue trying
                        thread::sleep(poll_interval);
                        continue;
                    }
                };

                // Skip empty reads
                if samples.is_empty() {
                    thread::sleep(poll_interval);
                    continue;
                }

                // Create audio frame
                let frame = AudioFrame::new(
                    samples,
                    Instant::now(),
                    audio_sequence.fetch_add(1, Ordering::Relaxed),
                );

                // Try to send - if channel is full, drop the frame
                if audio_tx.try_send(frame).is_err() {
                    // Channel full or disconnected
                    if !audio_running.load(Ordering::SeqCst) {
                        break;
                    }
                }

                thread::sleep(poll_interval);
            }

            // Stop audio capture
            let _ = audio_source.stop();
        });

        // Collect all thread handles
        let mut threads = vec![audio_handle, drain_handle];

        // Wrap runner join handles
        threads.push(thread::spawn(move || {
            let _ = vad_runner.join();
        }));
        threads.push(thread::spawn(move || {
            let _ = chunker_runner.join();
        }));
        threads.push(thread::spawn(move || {
            let _ = transcriber_runner.join();
        }));
        threads.push(thread::spawn(move || {
            let _ = injector_runner.join();
        }));

        Ok(ContinuousPipelineHandle { running, threads })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ContinuousPipelineConfig::default();
        assert_eq!(config.audio_buffer, 32);
        assert_eq!(config.sample_rate, 16000);
        assert_eq!(config.vad_buffer, 16);
        assert_eq!(config.chunk_buffer, 4);
        assert_eq!(config.transcribe_buffer, 4);
        assert!(!config.show_levels);
        assert!(config.auto_level);
        assert!(!config.quiet);
    }

    #[test]
    fn test_pipeline_creation() {
        let config = ContinuousPipelineConfig::default();
        let pipeline = ContinuousPipeline::new(config);
        // Verify it compiles and doesn't panic
        drop(pipeline);
    }

    #[test]
    fn test_pipeline_with_custom_error_reporter() {
        let config = ContinuousPipelineConfig::default();
        let reporter = Arc::new(LogReporter);
        let pipeline = ContinuousPipeline::new(config).with_error_reporter(reporter);
        drop(pipeline);
    }

    #[test]
    fn test_config_builder_pattern() {
        let config = ContinuousPipelineConfig {
            show_levels: true,
            auto_level: false,
            quiet: true,
            sample_rate: 48000,
            audio_buffer: 64,
            ..Default::default()
        };

        assert!(config.show_levels);
        assert!(!config.auto_level);
        assert!(config.quiet);
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.audio_buffer, 64);
    }

    #[test]
    fn test_handle_is_running() {
        let running = Arc::new(AtomicBool::new(true));
        let handle = ContinuousPipelineHandle {
            running: running.clone(),
            threads: vec![],
        };

        assert!(handle.is_running());

        running.store(false, Ordering::SeqCst);
        assert!(!handle.is_running());
    }
}
