//! Audio pipeline that runs from startup until shutdown.

use crate::audio::recorder::AudioSource;
use crate::audio::vad::{Clock, SystemClock, VadConfig};
use crate::error::Result;
use crate::pipeline::adaptive_chunker::AdaptiveChunkerConfig;
use crate::pipeline::error::{ErrorReporter, LogReporter};
use crate::pipeline::sink::{SinkStation, TextSink};
use crate::pipeline::station::StationRunner;
use crate::pipeline::types::AudioFrame;
use crate::pipeline::{ChunkerStation, TranscriberStation, VadStation};
use crate::stt::transcriber::Transcriber;
use crossbeam_channel::bounded;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Configuration for the pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// VAD configuration
    pub vad: VadConfig,
    /// Chunker configuration
    pub chunker: AdaptiveChunkerConfig,
    /// Verbosity level (0=quiet results, 1=meter+results, 2=full diagnostics)
    pub verbosity: u8,
    /// Enable auto-leveling for VAD
    pub auto_level: bool,
    /// Suppress output messages
    pub quiet: bool,
    /// Sample rate
    pub sample_rate: u32,
    /// Channel buffer sizes
    pub audio_buffer: usize,
    pub vad_buffer: usize,
    pub chunk_buffer: usize,
    pub transcribe_buffer: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            vad: VadConfig::default(),
            chunker: AdaptiveChunkerConfig::default(),
            verbosity: 0,
            auto_level: true,
            quiet: false,
            sample_rate: 16000,
            audio_buffer: 32,
            vad_buffer: 16,
            chunk_buffer: 4,
            transcribe_buffer: 4,
        }
    }
}

/// Handle to a running pipeline.
pub struct PipelineHandle {
    /// Flag to signal shutdown
    running: Arc<AtomicBool>,
    /// Join handles for spawned threads
    threads: Vec<JoinHandle<()>>,
    /// Receiver for sink's finish() result
    result_rx: Option<crossbeam_channel::Receiver<Option<String>>>,
}

impl PipelineHandle {
    /// Stops the pipeline gracefully and returns the sink's accumulated result.
    pub fn stop(mut self) -> Option<String> {
        // Signal shutdown
        self.running.store(false, Ordering::SeqCst);

        // Wait for all threads to complete
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }

        // Receive sink's finish() result
        self.result_rx.and_then(|rx| rx.recv().ok().flatten())
    }

    /// Returns true if the pipeline is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Audio pipeline: AudioSource → VAD → Chunker → Transcriber → TextSink.
pub struct Pipeline {
    config: PipelineConfig,
    error_reporter: Arc<dyn ErrorReporter>,
    clock: Arc<dyn Clock>,
}

impl Pipeline {
    /// Creates a new pipeline with default error reporter.
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            error_reporter: Arc::new(LogReporter),
            clock: Arc::new(SystemClock),
        }
    }

    /// Sets a custom error reporter.
    pub fn with_error_reporter(mut self, reporter: Arc<dyn ErrorReporter>) -> Self {
        self.error_reporter = reporter;
        self
    }

    /// Sets a custom clock (for deterministic testing).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Starts the pipeline.
    ///
    /// # Arguments
    /// * `audio_source` - Audio capture source
    /// * `transcriber` - Transcriber for speech-to-text
    /// * `sink` - Text output handler (injector, collector, etc.)
    ///
    /// # Returns
    /// Handle to control and stop the pipeline
    pub fn start(
        self,
        mut audio_source: Box<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        sink: Box<dyn TextSink>,
    ) -> Result<PipelineHandle> {
        let running = Arc::new(AtomicBool::new(true));
        let sequence = Arc::new(AtomicU64::new(0));

        // Create channels between stations
        let (audio_tx, audio_rx) = bounded(self.config.audio_buffer);
        let (vad_tx, vad_rx) = bounded(self.config.vad_buffer);
        let (chunk_tx, chunk_rx) = bounded(self.config.chunk_buffer);
        let (transcribe_tx, transcribe_rx) = bounded(self.config.transcribe_buffer);

        // Create stations
        let vad_station = VadStation::with_clock(self.config.vad, self.clock.clone())
            .with_show_levels(self.config.verbosity >= 1)
            .with_auto_level(self.config.auto_level)
            .with_sample_rate(self.config.sample_rate);

        let chunker_station = ChunkerStation::with_clock(self.config.chunker, self.clock.clone())
            .with_sample_rate(self.config.sample_rate)
            .with_verbose(self.config.verbosity >= 2);

        let transcriber_station =
            TranscriberStation::new(transcriber).with_verbose(self.config.verbosity >= 2);

        // Create sink station with result channel
        let (result_tx, result_rx) = bounded(1);
        let sink_station =
            SinkStation::new(sink, self.config.quiet, self.config.verbosity, result_tx);

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
        let (sink_out_tx, sink_out_rx) = bounded::<()>(self.config.transcribe_buffer);

        let sink_runner = StationRunner::spawn(
            sink_station,
            transcribe_rx,
            sink_out_tx,
            self.error_reporter.clone(),
        );

        // Drain the sink output in a separate thread
        let drain_running = running.clone();
        let drain_handle = thread::spawn(move || {
            while drain_running.load(Ordering::SeqCst) {
                if sink_out_rx
                    .recv_timeout(Duration::from_millis(100))
                    .is_err()
                {
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

                // Source exhausted — exit polling loop.
                if samples.is_empty() {
                    break;
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
            let _ = sink_runner.join();
        }));

        Ok(PipelineHandle {
            running,
            threads,
            result_rx: Some(result_rx),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recorder::{FramePhase, MockAudioSource};
    use crate::audio::vad::MockClock;
    use crate::error::{Result, VoicshError};
    use crate::pipeline::sink::CollectorSink;
    use crate::stt::transcriber::MockTranscriber;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    // Test audio source that produces samples and can be stopped
    struct TestAudioSource {
        samples: Vec<i16>,
        started: Arc<Mutex<bool>>,
        stopped: Arc<Mutex<bool>>,
        read_count: Arc<AtomicU32>,
        max_reads: u32,
    }

    impl TestAudioSource {
        fn new(samples: Vec<i16>, max_reads: u32) -> Self {
            Self {
                samples,
                started: Arc::new(Mutex::new(false)),
                stopped: Arc::new(Mutex::new(false)),
                read_count: Arc::new(AtomicU32::new(0)),
                max_reads,
            }
        }
    }

    impl crate::audio::recorder::AudioSource for TestAudioSource {
        fn start(&mut self) -> Result<()> {
            *self.started.lock().unwrap() = true;
            Ok(())
        }

        fn stop(&mut self) -> Result<()> {
            *self.stopped.lock().unwrap() = true;
            Ok(())
        }

        fn read_samples(&mut self) -> Result<Vec<i16>> {
            let count = self.read_count.fetch_add(1, Ordering::Relaxed);
            if count >= self.max_reads {
                // Return empty to signal we're done
                Ok(Vec::new())
            } else {
                Ok(self.samples.clone())
            }
        }
    }

    // Failing audio source for error testing
    struct FailingAudioSource {
        error_message: String,
    }

    impl FailingAudioSource {
        fn new(message: &str) -> Self {
            Self {
                error_message: message.to_string(),
            }
        }
    }

    impl crate::audio::recorder::AudioSource for FailingAudioSource {
        fn start(&mut self) -> Result<()> {
            Err(VoicshError::AudioCapture {
                message: self.error_message.clone(),
            })
        }

        fn stop(&mut self) -> Result<()> {
            Ok(())
        }

        fn read_samples(&mut self) -> Result<Vec<i16>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn test_config_default() {
        let config = PipelineConfig::default();
        assert_eq!(config.audio_buffer, 32);
        assert_eq!(config.sample_rate, 16000);
        assert_eq!(config.vad_buffer, 16);
        assert_eq!(config.chunk_buffer, 4);
        assert_eq!(config.transcribe_buffer, 4);
        assert_eq!(config.verbosity, 0);
        assert!(config.auto_level);
        assert!(!config.quiet);
    }

    #[test]
    fn test_pipeline_creation() {
        let config = PipelineConfig::default();
        let pipeline = Pipeline::new(config);
        // Verify it compiles and doesn't panic
        drop(pipeline);
    }

    #[test]
    fn test_pipeline_with_custom_error_reporter() {
        let config = PipelineConfig::default();
        let reporter = Arc::new(LogReporter);
        let pipeline = Pipeline::new(config).with_error_reporter(reporter);
        drop(pipeline);
    }

    #[test]
    fn test_config_builder_pattern() {
        let config = PipelineConfig {
            verbosity: 2,
            auto_level: false,
            quiet: true,
            sample_rate: 48000,
            audio_buffer: 64,
            ..Default::default()
        };

        assert_eq!(config.verbosity, 2);
        assert!(!config.auto_level);
        assert!(config.quiet);
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.audio_buffer, 64);
    }

    #[test]
    fn test_handle_is_running() {
        let running = Arc::new(AtomicBool::new(true));
        let handle = PipelineHandle {
            running: running.clone(),
            threads: vec![],
            result_rx: None,
        };

        assert!(handle.is_running());

        running.store(false, Ordering::SeqCst);
        assert!(!handle.is_running());
    }

    #[test]
    fn test_handle_stop_returns_none_without_result() {
        let running = Arc::new(AtomicBool::new(true));
        let handle = PipelineHandle {
            running,
            threads: vec![],
            result_rx: None,
        };

        let result = handle.stop();
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_stop_sets_running_false() {
        let running = Arc::new(AtomicBool::new(true));
        let (result_tx, result_rx) = bounded(1);
        result_tx.send(Some("test".to_string())).unwrap();
        drop(result_tx);

        let handle = PipelineHandle {
            running: running.clone(),
            threads: vec![],
            result_rx: Some(result_rx),
        };

        assert!(running.load(Ordering::SeqCst));
        let _result = handle.stop();
        assert!(!running.load(Ordering::SeqCst));
    }

    #[test]
    fn test_handle_stop_returns_result_from_channel() {
        let running = Arc::new(AtomicBool::new(true));
        let (result_tx, result_rx) = bounded(1);
        result_tx.send(Some("collected text".to_string())).unwrap();
        drop(result_tx);

        let handle = PipelineHandle {
            running,
            threads: vec![],
            result_rx: Some(result_rx),
        };

        let result = handle.stop();
        assert_eq!(result, Some("collected text".to_string()));
    }

    #[test]
    fn test_handle_stop_returns_none_when_channel_disconnected() {
        let running = Arc::new(AtomicBool::new(true));
        let (result_tx, result_rx) = bounded::<Option<String>>(1);
        // Drop sender immediately so recv returns Err(disconnected)
        drop(result_tx);

        let handle = PipelineHandle {
            running,
            threads: vec![],
            result_rx: Some(result_rx),
        };

        let result = handle.stop();
        assert!(result.is_none());
    }

    #[test]
    fn test_pipeline_start_audio_source_fails() {
        let config = PipelineConfig::default();
        let pipeline = Pipeline::new(config);

        let audio_source = Box::new(FailingAudioSource::new("audio init failed"));
        let transcriber = Arc::new(MockTranscriber::new("test-model"));
        let sink = Box::new(CollectorSink::new());

        let result = pipeline.start(audio_source, transcriber, sink);
        assert!(result.is_err());

        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert_eq!(message, "audio init failed");
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_pipeline_start_and_stop_integration() {
        let config = PipelineConfig {
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };
        let pipeline = Pipeline::new(config);

        // Create audio source with minimal samples
        let samples = vec![1000i16; 160];
        let audio_source = Box::new(TestAudioSource::new(samples, 2));

        // Create transcriber that returns "hello"
        let transcriber = Arc::new(MockTranscriber::new("test-model").with_response("hello"));

        // Create collector sink
        let sink = Box::new(CollectorSink::new());

        // Start pipeline
        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();

        // Verify pipeline is running
        assert!(handle.is_running());

        // Stop pipeline immediately (minimal runtime)
        let _result = handle.stop();
    }

    #[test]
    fn test_pipeline_start_creates_running_handle() {
        // This test verifies handle creation without actually waiting for threads
        let running = Arc::new(AtomicBool::new(true));
        let (_, result_rx) = bounded::<Option<String>>(1);

        let handle = PipelineHandle {
            running: running.clone(),
            threads: vec![],
            result_rx: Some(result_rx),
        };

        // Handle should report running
        assert!(handle.is_running());

        // Stop should set running to false
        drop(handle);
    }

    #[test]
    fn test_pipeline_full_cycle() {
        // Full integration test with mock clock and frame sequence.
        // Audio thread sleeps 16ms per frame, so we keep counts low for speed.
        let mock_clock = Arc::new(MockClock::new());

        let config = PipelineConfig {
            vad: VadConfig {
                speech_threshold: 0.02,
                silence_duration_ms: 200,
                min_speech_ms: 50,
                ..Default::default()
            },
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config).with_clock(mock_clock.clone());

        // Each frame = 160 samples at 16kHz = 10ms audio.
        // Audio thread sleeps 16ms/frame, so 15 frames ≈ 240ms wall time.
        let loud_phase = FramePhase {
            samples: vec![10000i16; 160],
            count: 15,
        };
        let quiet_phase = FramePhase {
            samples: vec![0i16; 160],
            count: 15,
        };

        let audio_source =
            Box::new(MockAudioSource::new().with_frame_sequence(vec![loud_phase, quiet_phase]));

        let transcriber = Arc::new(MockTranscriber::new("test-model").with_response("hello"));
        let sink = Box::new(CollectorSink::new());

        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();
        assert!(handle.is_running());

        // Let all 30 frames push (30 * 16ms ≈ 480ms), advance clock in steps
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(200));
            mock_clock.advance(Duration::from_millis(400));
        }

        let result = handle.stop();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn test_pipeline_with_mock_audio_source() {
        // Integration test: verifies pipeline processes frames and returns transcription.
        let mock_clock = Arc::new(MockClock::new());

        let config = PipelineConfig {
            vad: VadConfig {
                speech_threshold: 0.02,
                silence_duration_ms: 200,
                min_speech_ms: 50,
                ..Default::default()
            },
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config).with_clock(mock_clock.clone());

        // Keep frame counts low: audio thread sleeps 16ms/frame
        let loud_phase = FramePhase {
            samples: vec![10000i16; 160],
            count: 15,
        };
        let quiet_phase = FramePhase {
            samples: vec![0i16; 160],
            count: 15,
        };

        let audio_source =
            Box::new(MockAudioSource::new().with_frame_sequence(vec![loud_phase, quiet_phase]));

        let transcriber = Arc::new(MockTranscriber::new("test-model").with_response("hello"));
        let sink = Box::new(CollectorSink::new());

        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();
        assert!(handle.is_running());

        // Let all 30 frames push (30 * 16ms ≈ 480ms), advance clock in steps
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(200));
            mock_clock.advance(Duration::from_millis(400));
        }

        let result = handle.stop();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn test_pipeline_audio_read_error_continues() {
        // Tests that audio read errors don't crash the pipeline
        let config = PipelineConfig {
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config);

        // Create audio source that fails on read
        let audio_source = Box::new(MockAudioSource::new().with_read_failure());

        let transcriber = Arc::new(MockTranscriber::new("test-model"));
        let sink = Box::new(CollectorSink::new());

        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();
        assert!(handle.is_running());

        // Sleep briefly to allow audio thread to attempt reads
        thread::sleep(Duration::from_millis(100));

        // Stop and verify no transcription produced
        let result = handle.stop();
        assert!(result.is_none());
    }

    #[test]
    fn test_pipeline_stop_without_transcription() {
        // Tests quick stop with no speech detected
        let mock_clock = Arc::new(MockClock::new());

        let config = PipelineConfig {
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config).with_clock(mock_clock.clone());

        // Create audio source with only quiet frames
        let quiet_phase = FramePhase {
            samples: vec![0i16; 160],
            count: 10,
        };

        let audio_source = Box::new(MockAudioSource::new().with_frame_sequence(vec![quiet_phase]));

        let transcriber = Arc::new(MockTranscriber::new("test-model"));
        let sink = Box::new(CollectorSink::new());

        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();
        assert!(handle.is_running());

        // Brief wait to allow some frames to flow
        thread::sleep(Duration::from_millis(50));
        mock_clock.advance(Duration::from_millis(50));

        // Stop immediately
        let result = handle.stop();
        assert!(result.is_none());
    }

    #[test]
    fn test_pipeline_quiet_only_no_transcription() {
        // Only quiet frames → VAD never detects speech → no chunks → no transcription
        let mock_clock = Arc::new(MockClock::new());

        let config = PipelineConfig {
            vad: VadConfig {
                speech_threshold: 0.02,
                silence_duration_ms: 200,
                min_speech_ms: 50,
                ..Default::default()
            },
            quiet: true,
            verbosity: 0,
            ..Default::default()
        };

        let pipeline = Pipeline::new(config).with_clock(mock_clock.clone());

        let quiet_phase = FramePhase {
            samples: vec![0i16; 160],
            count: 15,
        };

        let audio_source = Box::new(MockAudioSource::new().with_frame_sequence(vec![quiet_phase]));
        let transcriber =
            Arc::new(MockTranscriber::new("test-model").with_response("should not appear"));
        let sink = Box::new(CollectorSink::new());

        let handle = pipeline.start(audio_source, transcriber, sink).unwrap();

        // Let frames flow and source exhaust
        thread::sleep(Duration::from_millis(100));
        mock_clock.advance(Duration::from_millis(500));
        thread::sleep(Duration::from_millis(100));

        let result = handle.stop();
        assert_eq!(
            result, None,
            "Quiet-only audio should produce no transcription"
        );
    }

    #[test]
    fn test_pipeline_verbose_modes() {
        // Verify pipeline starts and stops cleanly with verbosity 1 and 2
        for verbosity in [1u8, 2] {
            let mock_clock = Arc::new(MockClock::new());

            let config = PipelineConfig {
                vad: VadConfig {
                    speech_threshold: 0.02,
                    silence_duration_ms: 200,
                    min_speech_ms: 50,
                    ..Default::default()
                },
                quiet: false,
                verbosity,
                ..Default::default()
            };

            let pipeline = Pipeline::new(config).with_clock(mock_clock.clone());

            let loud_phase = FramePhase {
                samples: vec![10000i16; 160],
                count: 15,
            };
            let quiet_phase = FramePhase {
                samples: vec![0i16; 160],
                count: 15,
            };

            let audio_source =
                Box::new(MockAudioSource::new().with_frame_sequence(vec![loud_phase, quiet_phase]));
            let transcriber = Arc::new(MockTranscriber::new("test-model").with_response("verbose"));
            let sink = Box::new(CollectorSink::new());

            let handle = pipeline.start(audio_source, transcriber, sink).unwrap();
            assert!(handle.is_running());

            for _ in 0..4 {
                thread::sleep(Duration::from_millis(200));
                mock_clock.advance(Duration::from_millis(400));
            }

            let result = handle.stop();
            assert_eq!(
                result,
                Some("verbose".to_string()),
                "Verbosity {verbosity} should produce transcription"
            );
        }
    }
}
