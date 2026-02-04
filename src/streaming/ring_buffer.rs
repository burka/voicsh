//! Ring buffer for continuous audio capture.
//!
//! Wraps an audio source and provides:
//! - Continuous recording without stopping
//! - Sample numbering for tracking
//! - Decoupled from transcription timing

use crate::audio::recorder::AudioSource;
use crate::error::Result;
use crate::streaming::frame::AudioFrame;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc;

/// Configuration for the ring buffer.
#[derive(Debug, Clone)]
pub struct RingBufferConfig {
    /// Channel buffer size (number of frames to buffer).
    pub channel_buffer_size: usize,
    /// Polling interval when no samples available (ms).
    pub poll_interval_ms: u64,
}

impl Default for RingBufferConfig {
    fn default() -> Self {
        Self {
            channel_buffer_size: 1000,
            poll_interval_ms: 10,
        }
    }
}

/// Ring buffer that continuously captures audio and emits frames.
pub struct RingBuffer<A: AudioSource> {
    audio_source: A,
    config: RingBufferConfig,
    sequence: AtomicU64,
    running: Arc<AtomicBool>,
}

impl<A: AudioSource + 'static> RingBuffer<A> {
    /// Creates a new ring buffer wrapping the given audio source.
    pub fn new(audio_source: A) -> Self {
        Self::with_config(audio_source, RingBufferConfig::default())
    }

    /// Creates a new ring buffer with custom configuration.
    pub fn with_config(audio_source: A, config: RingBufferConfig) -> Self {
        Self {
            audio_source,
            config,
            sequence: AtomicU64::new(0),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Starts continuous audio capture in a background thread.
    ///
    /// Returns a receiver for audio frames. The capture runs until
    /// `stop()` is called or the receiver is dropped.
    pub fn start(mut self) -> Result<(mpsc::Receiver<AudioFrame>, RingBufferHandle)> {
        let (tx, rx) = mpsc::channel(self.config.channel_buffer_size);
        let running = self.running.clone();

        // Start audio capture
        self.audio_source.start()?;
        running.store(true, Ordering::SeqCst);

        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);

        // Spawn capture thread
        thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                match self.audio_source.read_samples() {
                    Ok(samples) if !samples.is_empty() => {
                        let seq = self.sequence.fetch_add(1, Ordering::SeqCst);
                        let frame = AudioFrame::new(seq, samples);

                        // Try to send, stop if receiver dropped
                        if tx.blocking_send(frame).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {
                        // No samples yet, wait briefly
                        thread::sleep(poll_interval);
                    }
                    Err(e) => {
                        eprintln!("Audio capture error: {}", e);
                        break;
                    }
                }
            }

            // Clean up
            let _ = self.audio_source.stop();
        });

        let handle = RingBufferHandle {
            running: self.running.clone(),
        };

        Ok((rx, handle))
    }
}

/// Handle to control a running ring buffer.
#[derive(Clone)]
pub struct RingBufferHandle {
    running: Arc<AtomicBool>,
}

impl RingBufferHandle {
    /// Stops the ring buffer capture.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Returns true if the ring buffer is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recorder::MockAudioSource;

    #[tokio::test]
    async fn test_ring_buffer_config_default() {
        let config = RingBufferConfig::default();
        assert_eq!(config.channel_buffer_size, 1000);
        assert_eq!(config.poll_interval_ms, 10);
    }

    #[tokio::test]
    async fn test_ring_buffer_creation() {
        let source = MockAudioSource::new();
        let buffer = RingBuffer::new(source);
        assert!(!buffer.running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_ring_buffer_handle_stop() {
        let source = MockAudioSource::new().with_samples(vec![100i16; 160]);
        let buffer = RingBuffer::new(source);

        let (mut rx, handle) = buffer.start().unwrap();
        assert!(handle.is_running());

        // Should receive at least one frame
        let frame = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .ok()
            .flatten();
        assert!(frame.is_some());

        // Stop and verify
        handle.stop();
        assert!(!handle.is_running());
    }

    #[tokio::test]
    async fn test_ring_buffer_sequence_numbers() {
        let source = MockAudioSource::new().with_samples(vec![100i16; 160]);
        let buffer = RingBuffer::new(source);

        let (mut rx, handle) = buffer.start().unwrap();

        // Collect a few frames
        let mut sequences = Vec::new();
        for _ in 0..3 {
            if let Ok(Some(frame)) =
                tokio::time::timeout(Duration::from_millis(100), rx.recv()).await
            {
                sequences.push(frame.sequence);
            }
        }

        handle.stop();

        // Verify sequences are monotonically increasing
        for i in 1..sequences.len() {
            assert!(
                sequences[i] > sequences[i - 1],
                "Sequences should increase: {:?}",
                sequences
            );
        }
    }

    #[tokio::test]
    async fn test_ring_buffer_start_failure() {
        let source = MockAudioSource::new().with_start_failure();
        let buffer = RingBuffer::new(source);

        let result = buffer.start();
        assert!(result.is_err());
    }
}
