//! Frame types for the streaming pipeline.
//!
//! Defines the data structures that flow between pipeline stations.

use std::time::Instant;

/// Audio frame with metadata for tracking through the pipeline.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Sequence number for ordering frames.
    pub sequence: u64,
    /// Timestamp when the audio was captured.
    pub timestamp: Instant,
    /// Audio samples as 16-bit PCM.
    pub samples: Vec<i16>,
}

impl AudioFrame {
    /// Creates a new audio frame.
    pub fn new(sequence: u64, samples: Vec<i16>) -> Self {
        Self {
            sequence,
            timestamp: Instant::now(),
            samples,
        }
    }

    /// Returns the duration of this frame in milliseconds.
    pub fn duration_ms(&self, sample_rate: u32) -> u32 {
        (self.samples.len() as u32 * 1000) / sample_rate
    }
}

/// Control events sent from silence detector to chunker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    /// Speech has started after silence.
    SpeechStart,
    /// Silence detected - flush current chunk immediately.
    FlushChunk,
    /// Speech has ended - final flush and stop.
    SpeechEnd,
}

/// A chunk of audio ready for transcription.
#[derive(Debug, Clone)]
pub struct ChunkData {
    /// Unique identifier for this chunk.
    pub chunk_id: u64,
    /// Sequence number of first audio frame in this chunk.
    pub start_sequence: u64,
    /// Sequence number of last audio frame in this chunk.
    pub end_sequence: u64,
    /// Combined audio samples for this chunk.
    pub samples: Vec<i16>,
    /// Whether this chunk was flushed early (on silence).
    pub flushed_early: bool,
    /// Whether this is the final chunk (speech ended).
    pub is_final: bool,
}

impl ChunkData {
    /// Returns the duration of this chunk in milliseconds.
    pub fn duration_ms(&self, sample_rate: u32) -> u32 {
        (self.samples.len() as u32 * 1000) / sample_rate
    }
}

/// Transcription result for a chunk.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// ID of the chunk that was transcribed.
    pub chunk_id: u64,
    /// Transcribed text.
    pub text: String,
    /// Whether this is the final result (speech ended).
    pub is_final: bool,
}

/// Unified frame type that can flow through the pipeline.
#[derive(Debug, Clone)]
pub enum PipelineFrame {
    /// Raw audio frame from ring buffer.
    Audio(AudioFrame),
    /// Control event from silence detector.
    Control(ControlEvent),
    /// Audio chunk ready for transcription.
    Chunk(ChunkData),
    /// Transcription result.
    Transcription(TranscriptionResult),
}

impl PipelineFrame {
    /// Returns true if this is an audio frame.
    pub fn is_audio(&self) -> bool {
        matches!(self, PipelineFrame::Audio(_))
    }

    /// Returns true if this is a control event.
    pub fn is_control(&self) -> bool {
        matches!(self, PipelineFrame::Control(_))
    }

    /// Returns true if this is a chunk.
    pub fn is_chunk(&self) -> bool {
        matches!(self, PipelineFrame::Chunk(_))
    }

    /// Returns true if this is a transcription result.
    pub fn is_transcription(&self) -> bool {
        matches!(self, PipelineFrame::Transcription(_))
    }

    /// Extracts the audio frame if this is an Audio variant.
    pub fn into_audio(self) -> Option<AudioFrame> {
        match self {
            PipelineFrame::Audio(f) => Some(f),
            _ => None,
        }
    }

    /// Extracts the control event if this is a Control variant.
    pub fn into_control(self) -> Option<ControlEvent> {
        match self {
            PipelineFrame::Control(e) => Some(e),
            _ => None,
        }
    }

    /// Extracts the chunk data if this is a Chunk variant.
    pub fn into_chunk(self) -> Option<ChunkData> {
        match self {
            PipelineFrame::Chunk(c) => Some(c),
            _ => None,
        }
    }

    /// Extracts the transcription result if this is a Transcription variant.
    pub fn into_transcription(self) -> Option<TranscriptionResult> {
        match self {
            PipelineFrame::Transcription(t) => Some(t),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_frame_creation() {
        let samples = vec![100i16, 200, 300];
        let frame = AudioFrame::new(42, samples.clone());

        assert_eq!(frame.sequence, 42);
        assert_eq!(frame.samples, samples);
    }

    #[test]
    fn test_audio_frame_duration() {
        let samples = vec![0i16; 16000]; // 1 second at 16kHz
        let frame = AudioFrame::new(0, samples);

        assert_eq!(frame.duration_ms(16000), 1000);
    }

    #[test]
    fn test_chunk_data_duration() {
        let samples = vec![0i16; 8000]; // 0.5 seconds at 16kHz
        let chunk = ChunkData {
            chunk_id: 1,
            start_sequence: 0,
            end_sequence: 10,
            samples,
            flushed_early: false,
            is_final: false,
        };

        assert_eq!(chunk.duration_ms(16000), 500);
    }

    #[test]
    fn test_pipeline_frame_variants() {
        let audio = PipelineFrame::Audio(AudioFrame::new(0, vec![0]));
        assert!(audio.is_audio());
        assert!(!audio.is_control());

        let control = PipelineFrame::Control(ControlEvent::FlushChunk);
        assert!(control.is_control());
        assert!(!control.is_audio());

        let chunk = PipelineFrame::Chunk(ChunkData {
            chunk_id: 0,
            start_sequence: 0,
            end_sequence: 0,
            samples: vec![],
            flushed_early: false,
            is_final: false,
        });
        assert!(chunk.is_chunk());

        let transcription = PipelineFrame::Transcription(TranscriptionResult {
            chunk_id: 0,
            text: "hello".to_string(),
            is_final: false,
        });
        assert!(transcription.is_transcription());
    }

    #[test]
    fn test_pipeline_frame_into_methods() {
        let frame = PipelineFrame::Audio(AudioFrame::new(5, vec![1, 2, 3]));
        let audio = frame.into_audio().unwrap();
        assert_eq!(audio.sequence, 5);

        let frame = PipelineFrame::Control(ControlEvent::SpeechStart);
        let control = frame.into_control().unwrap();
        assert_eq!(control, ControlEvent::SpeechStart);

        let frame = PipelineFrame::Audio(AudioFrame::new(0, vec![]));
        assert!(frame.into_control().is_none());
    }

    #[test]
    fn test_control_event_equality() {
        assert_eq!(ControlEvent::SpeechStart, ControlEvent::SpeechStart);
        assert_eq!(ControlEvent::FlushChunk, ControlEvent::FlushChunk);
        assert_eq!(ControlEvent::SpeechEnd, ControlEvent::SpeechEnd);
        assert_ne!(ControlEvent::SpeechStart, ControlEvent::FlushChunk);
    }
}
