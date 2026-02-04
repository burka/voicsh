//! Data types for the continuous audio pipeline.

use std::time::Instant;

/// A frame of raw audio samples with timing information.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// PCM samples (16-bit signed integers).
    pub samples: Vec<i16>,
    /// Timestamp when this frame was captured.
    pub timestamp: Instant,
    /// Sequence number for ordering and gap detection.
    pub sequence: u64,
}

impl AudioFrame {
    /// Creates a new audio frame.
    pub fn new(samples: Vec<i16>, timestamp: Instant, sequence: u64) -> Self {
        Self {
            samples,
            timestamp,
            sequence,
        }
    }
}

/// An audio frame with voice activity detection results.
#[derive(Debug, Clone)]
pub struct VadFrame {
    /// PCM samples (16-bit signed integers).
    pub samples: Vec<i16>,
    /// Timestamp when this frame was captured.
    pub timestamp: Instant,
    /// Whether speech was detected in this frame.
    pub is_speech: bool,
    /// Voice activity level (0.0 = silence, 1.0 = full speech).
    pub level: f32,
}

impl VadFrame {
    /// Creates a new VAD frame.
    pub fn new(samples: Vec<i16>, timestamp: Instant, is_speech: bool, level: f32) -> Self {
        Self {
            samples,
            timestamp,
            is_speech,
            level,
        }
    }
}

/// A chunk of audio ready for transcription.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// PCM samples (16-bit signed integers).
    pub samples: Vec<i16>,
    /// Duration of the chunk in milliseconds.
    pub duration_ms: u32,
    /// Sequence number for ordering.
    pub sequence: u64,
}

impl AudioChunk {
    /// Creates a new audio chunk.
    pub fn new(samples: Vec<i16>, duration_ms: u32, sequence: u64) -> Self {
        Self {
            samples,
            duration_ms,
            sequence,
        }
    }
}

/// Transcribed text with timing information.
#[derive(Debug, Clone)]
pub struct TranscribedText {
    /// The transcribed text.
    pub text: String,
    /// Timestamp when transcription completed.
    pub timestamp: Instant,
}

impl TranscribedText {
    /// Creates a new transcribed text result.
    pub fn new(text: String, timestamp: Instant) -> Self {
        Self { text, timestamp }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_frame_creation() {
        let samples = vec![100, 200, 300];
        let timestamp = Instant::now();
        let sequence = 42;

        let frame = AudioFrame::new(samples.clone(), timestamp, sequence);

        assert_eq!(frame.samples, samples);
        assert_eq!(frame.timestamp, timestamp);
        assert_eq!(frame.sequence, sequence);
    }

    #[test]
    fn test_vad_frame_creation() {
        let samples = vec![100, 200, 300];
        let timestamp = Instant::now();

        let frame = VadFrame::new(samples.clone(), timestamp, true, 0.8);

        assert_eq!(frame.samples, samples);
        assert_eq!(frame.timestamp, timestamp);
        assert!(frame.is_speech);
        assert!((frame.level - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_audio_chunk_creation() {
        let samples = vec![100, 200, 300];
        let chunk = AudioChunk::new(samples.clone(), 1000, 5);

        assert_eq!(chunk.samples, samples);
        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.sequence, 5);
    }

    #[test]
    fn test_transcribed_text_creation() {
        let timestamp = Instant::now();
        let text = TranscribedText::new("Hello world".to_string(), timestamp);

        assert_eq!(text.text, "Hello world");
        assert_eq!(text.timestamp, timestamp);
    }
}
