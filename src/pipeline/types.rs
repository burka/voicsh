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
    /// Timestamp when VAD processing started for this frame.
    pub vad_start: Instant,
}

impl VadFrame {
    /// Creates a new VAD frame.
    pub fn new(samples: Vec<i16>, timestamp: Instant, is_speech: bool, level: f32) -> Self {
        Self {
            samples,
            timestamp,
            is_speech,
            level,
            vad_start: Instant::now(),
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
    /// Timestamp when the first frame in this chunk was captured.
    pub capture_start: Instant,
    /// Timestamp when VAD processing began.
    pub vad_start: Instant,
    /// Timestamp when this chunk was created (after chunking).
    pub chunk_created: Instant,
}

impl AudioChunk {
    /// Creates a new audio chunk.
    pub fn new(samples: Vec<i16>, duration_ms: u32, sequence: u64) -> Self {
        let now = Instant::now();
        Self {
            samples,
            duration_ms,
            sequence,
            capture_start: now,
            vad_start: now,
            chunk_created: now,
        }
    }

    /// Creates a new audio chunk with timing information.
    pub fn with_timing(
        samples: Vec<i16>,
        duration_ms: u32,
        sequence: u64,
        capture_start: Instant,
        vad_start: Instant,
    ) -> Self {
        Self {
            samples,
            duration_ms,
            sequence,
            capture_start,
            vad_start,
            chunk_created: Instant::now(),
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
    /// Timestamp when the first frame was captured.
    pub capture_start: Instant,
    /// Timestamp when VAD processing began.
    pub vad_start: Instant,
    /// Timestamp when the chunk was created.
    pub chunk_created: Instant,
}

impl TranscribedText {
    /// Creates a new transcribed text result.
    pub fn new(text: String, timestamp: Instant) -> Self {
        let now = Instant::now();
        Self {
            text,
            timestamp,
            capture_start: now,
            vad_start: now,
            chunk_created: now,
        }
    }

    /// Creates a new transcribed text result with timing information.
    pub fn with_timing(
        text: String,
        capture_start: Instant,
        vad_start: Instant,
        chunk_created: Instant,
    ) -> Self {
        Self {
            text,
            timestamp: Instant::now(),
            capture_start,
            vad_start,
            chunk_created,
        }
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
        // vad_start is set to now, so just verify it exists
        assert!(frame.vad_start <= Instant::now());
    }

    #[test]
    fn test_audio_chunk_creation() {
        let samples = vec![100, 200, 300];
        let chunk = AudioChunk::new(samples.clone(), 1000, 5);

        assert_eq!(chunk.samples, samples);
        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.sequence, 5);
        // Timing fields are set to now
        assert!(chunk.capture_start <= Instant::now());
        assert!(chunk.vad_start <= Instant::now());
        assert!(chunk.chunk_created <= Instant::now());
    }

    #[test]
    fn test_transcribed_text_creation() {
        let timestamp = Instant::now();
        let text = TranscribedText::new("Hello world".to_string(), timestamp);

        assert_eq!(text.text, "Hello world");
        assert_eq!(text.timestamp, timestamp);
        // Timing fields are set to now
        assert!(text.capture_start <= Instant::now());
        assert!(text.vad_start <= Instant::now());
        assert!(text.chunk_created <= Instant::now());
    }

    #[test]
    fn test_audio_chunk_with_timing() {
        let samples = vec![100, 200, 300];
        let capture = Instant::now();
        let vad = Instant::now();
        let chunk = AudioChunk::with_timing(samples.clone(), 1000, 5, capture, vad);

        assert_eq!(chunk.samples, samples);
        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.sequence, 5);
        assert_eq!(chunk.capture_start, capture);
        assert_eq!(chunk.vad_start, vad);
        assert!(chunk.chunk_created >= vad);
    }

    #[test]
    fn test_transcribed_text_with_timing() {
        let capture = Instant::now();
        let vad = Instant::now();
        let chunk_created = Instant::now();
        let text =
            TranscribedText::with_timing("Hello world".to_string(), capture, vad, chunk_created);

        assert_eq!(text.text, "Hello world");
        assert_eq!(text.capture_start, capture);
        assert_eq!(text.vad_start, vad);
        assert_eq!(text.chunk_created, chunk_created);
        assert!(text.timestamp >= chunk_created);
    }
}
