//! Transcriber station that converts audio chunks to text via Whisper.

use crate::pipeline::error::{StationError, eprintln_clear};
use crate::pipeline::station::Station;
use crate::pipeline::types::{AudioChunk, TranscribedText};
use crate::stt::transcriber::Transcriber;
use std::sync::Arc;
use std::time::Instant;

/// Filters common Whisper markers and noise indicators from transcribed text.
fn clean_transcription(text: &str) -> String {
    let markers = [
        "[BLANK_AUDIO]",
        "[INAUDIBLE]",
        "[MUSIC]",
        "[APPLAUSE]",
        "[LAUGHTER]",
        "(BLANK_AUDIO)",
        "(inaudible)",
        "[silence]",
        "[noise]",
    ];

    let mut cleaned = text.to_string();
    for marker in markers {
        cleaned = cleaned.replace(marker, "");
    }
    cleaned.trim().to_string()
}

/// Station that transcribes audio chunks using a Whisper transcriber.
pub struct TranscriberStation {
    transcriber: Arc<dyn Transcriber>,
    verbose: bool,
}

impl TranscriberStation {
    /// Creates a new transcriber station.
    pub fn new(transcriber: Arc<dyn Transcriber>) -> Self {
        Self {
            transcriber,
            verbose: false,
        }
    }

    /// Configure whether to enable diagnostic output to stderr.
    ///
    /// When verbose is true, diagnostic info is logged during transcription.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

impl Station for TranscriberStation {
    type Input = AudioChunk;
    type Output = TranscribedText;

    fn name(&self) -> &'static str {
        "transcriber"
    }

    fn process(&mut self, chunk: AudioChunk) -> Result<Option<TranscribedText>, StationError> {
        // Log transcription start if verbose
        if self.verbose {
            eprintln_clear(&format!("  [transcribing {}ms...]", chunk.duration_ms));
        }

        // Attempt transcription
        let raw_text = self
            .transcriber
            .transcribe(&chunk.samples)
            .map_err(|e| StationError::Recoverable(format!("Transcription failed: {}", e)))?;

        // Clean Whisper markers
        let cleaned_text = clean_transcription(&raw_text);

        // Skip empty results
        if cleaned_text.is_empty() {
            return Ok(None);
        }

        // Log transcription completion if verbose
        if self.verbose {
            eprintln_clear(&format!("  [transcribed: {} chars]", cleaned_text.len()));
        }

        // Return transcribed text with current timestamp
        Ok(Some(TranscribedText::new(cleaned_text, Instant::now())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Result, VoicshError};

    struct MockTranscriber {
        response: String,
        should_fail: bool,
    }

    impl Transcriber for MockTranscriber {
        fn transcribe(&self, _samples: &[i16]) -> Result<String> {
            if self.should_fail {
                Err(VoicshError::Transcription {
                    message: "Mock error".to_string(),
                })
            } else {
                Ok(self.response.clone())
            }
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        fn is_ready(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_successful_transcription() {
        let transcriber = Arc::new(MockTranscriber {
            response: "Hello world".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3, 4, 5], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert_eq!(text.text, "Hello world");
    }

    #[test]
    fn test_error_handling_returns_recoverable() {
        let transcriber = Arc::new(MockTranscriber {
            response: String::new(),
            should_fail: true,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3, 4, 5], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_err());
        match result {
            Err(StationError::Recoverable(msg)) => {
                assert!(msg.contains("Transcription failed"));
                assert!(msg.contains("Mock error"));
            }
            _ => panic!("Expected Recoverable error"),
        }
    }

    #[test]
    fn test_whisper_marker_filtering() {
        let transcriber = Arc::new(MockTranscriber {
            response: "Hello [BLANK_AUDIO] world [INAUDIBLE] test".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert_eq!(text.text, "Hello  world  test");
    }

    #[test]
    fn test_multiple_markers_filtered() {
        let transcriber = Arc::new(MockTranscriber {
            response: "[MUSIC] [APPLAUSE] Speech here [LAUGHTER] more speech [noise]".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert_eq!(text.text, "Speech here  more speech");
    }

    #[test]
    fn test_empty_result_returns_none() {
        let transcriber = Arc::new(MockTranscriber {
            response: String::new(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_whitespace_only_returns_none() {
        let transcriber = Arc::new(MockTranscriber {
            response: "   \n\t  ".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_markers_only_returns_none() {
        let transcriber = Arc::new(MockTranscriber {
            response: "[BLANK_AUDIO] [INAUDIBLE] [silence]".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_clean_transcription_removes_all_markers() {
        let input = "[BLANK_AUDIO] text [INAUDIBLE] more [MUSIC] [APPLAUSE] [LAUGHTER] (BLANK_AUDIO) (inaudible) [silence] [noise]";
        let result = clean_transcription(input);
        assert_eq!(result, "text  more");
    }

    #[test]
    fn test_clean_transcription_preserves_normal_text() {
        let input = "This is normal text without markers";
        let result = clean_transcription(input);
        assert_eq!(result, "This is normal text without markers");
    }

    #[test]
    fn test_clean_transcription_handles_empty_string() {
        let result = clean_transcription("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_clean_transcription_trims_whitespace() {
        let input = "  text with spaces  ";
        let result = clean_transcription(input);
        assert_eq!(result, "text with spaces");
    }

    #[test]
    fn test_station_name() {
        let transcriber = Arc::new(MockTranscriber {
            response: String::new(),
            should_fail: false,
        });

        let station = TranscriberStation::new(transcriber);
        assert_eq!(station.name(), "transcriber");
    }

    #[test]
    fn test_timestamp_is_current() {
        let transcriber = Arc::new(MockTranscriber {
            response: "Test text".to_string(),
            should_fail: false,
        });

        let mut station = TranscriberStation::new(transcriber);

        let before = Instant::now();
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);
        let after = Instant::now();

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();

        // Timestamp should be between before and after
        assert!(text.timestamp >= before);
        assert!(text.timestamp <= after);
    }
}
