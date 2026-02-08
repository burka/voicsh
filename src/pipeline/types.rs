//! Data types for the continuous audio pipeline.

use std::time::Instant;

/// Timing information for pipeline stages (only populated when verbosity >= 1).
#[derive(Debug, Clone)]
pub struct ChunkTiming {
    /// Timestamp when the first frame in this chunk was captured.
    pub capture_start: Instant,
    /// Timestamp when VAD processing began.
    pub vad_start: Instant,
    /// Timestamp when this chunk was created (after chunking).
    pub chunk_created: Instant,
}

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
    /// Timestamp when VAD processing started for this frame (only when verbosity >= 1).
    pub vad_start: Option<Instant>,
}

impl VadFrame {
    /// Creates a new VAD frame.
    pub fn new(samples: Vec<i16>, timestamp: Instant, is_speech: bool, level: f32) -> Self {
        Self {
            samples,
            timestamp,
            is_speech,
            level,
            vad_start: None,
        }
    }

    /// Creates a new VAD frame with timing information.
    pub fn with_timing(
        samples: Vec<i16>,
        timestamp: Instant,
        is_speech: bool,
        level: f32,
        vad_start: Instant,
    ) -> Self {
        Self {
            samples,
            timestamp,
            is_speech,
            level,
            vad_start: Some(vad_start),
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
    /// Timing information (only populated when verbosity >= 1).
    pub timing: Option<Box<ChunkTiming>>,
}

impl AudioChunk {
    /// Creates a new audio chunk without timing information.
    pub fn new(samples: Vec<i16>, duration_ms: u32, sequence: u64) -> Self {
        Self {
            samples,
            duration_ms,
            sequence,
            timing: None,
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
            timing: Some(Box::new(ChunkTiming {
                capture_start,
                vad_start,
                chunk_created: Instant::now(),
            })),
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
    /// Timing information (only populated when verbosity >= 1).
    pub timing: Option<Box<ChunkTiming>>,
}

impl TranscribedText {
    /// Creates a new transcribed text result without timing information.
    pub fn new(text: String) -> Self {
        Self {
            text,
            timestamp: Instant::now(),
            timing: None,
        }
    }

    /// Creates a new transcribed text result with timing information.
    pub fn with_timing(text: String, timing: Option<Box<ChunkTiming>>) -> Self {
        Self {
            text,
            timestamp: Instant::now(),
            timing,
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
        assert!(frame.vad_start.is_none());
    }

    #[test]
    fn test_vad_frame_with_timing() {
        let samples = vec![100, 200, 300];
        let timestamp = Instant::now();
        let vad_start = Instant::now();

        let frame = VadFrame::with_timing(samples.clone(), timestamp, true, 0.8, vad_start);

        assert_eq!(frame.samples, samples);
        assert_eq!(frame.timestamp, timestamp);
        assert!(frame.is_speech);
        assert!((frame.level - 0.8).abs() < f32::EPSILON);
        assert_eq!(frame.vad_start, Some(vad_start));
    }

    #[test]
    fn test_audio_chunk_creation() {
        let samples = vec![100, 200, 300];
        let chunk = AudioChunk::new(samples.clone(), 1000, 5);

        assert_eq!(chunk.samples, samples);
        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.sequence, 5);
        assert!(chunk.timing.is_none());
    }

    #[test]
    fn test_transcribed_text_creation() {
        let text = TranscribedText::new("Hello world".to_string());

        assert_eq!(text.text, "Hello world");
        assert!(text.timestamp <= Instant::now());
        assert!(text.timing.is_none());
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
        assert!(chunk.timing.is_some());
        let timing = chunk.timing.unwrap();
        assert_eq!(timing.capture_start, capture);
        assert_eq!(timing.vad_start, vad);
        assert!(timing.chunk_created >= vad);
    }

    #[test]
    fn test_transcribed_text_with_timing() {
        let capture = Instant::now();
        let vad = Instant::now();
        let chunk_created = Instant::now();
        let timing = Some(Box::new(ChunkTiming {
            capture_start: capture,
            vad_start: vad,
            chunk_created,
        }));
        let text = TranscribedText::with_timing("Hello world".to_string(), timing);

        assert_eq!(text.text, "Hello world");
        assert!(text.timing.is_some());
        let timing = text.timing.unwrap();
        assert_eq!(timing.capture_start, capture);
        assert_eq!(timing.vad_start, vad);
        assert_eq!(timing.chunk_created, chunk_created);
        assert!(text.timestamp >= chunk_created);
    }
}
