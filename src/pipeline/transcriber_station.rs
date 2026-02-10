//! Transcriber station that converts audio chunks to text via Whisper.

use crate::pipeline::error::{StationError, eprintln_clear};
use crate::pipeline::station::Station;
use crate::pipeline::types::{AudioChunk, TranscribedText};
use crate::stt::transcriber::Transcriber;
use std::sync::Arc;
use std::time::Instant;

/// Strips Whisper non-speech annotations in any language.
///
/// Whisper wraps annotations in `[…]`, `*…*`, or `(…)` — these never contain
/// real speech. Unmatched opening delimiters are kept as-is.
fn clean_transcription(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '[' | '(' | '*' => {
                let close = match ch {
                    '[' => ']',
                    '(' => ')',
                    '*' => '*',
                    _ => unreachable!(),
                };
                chars.next(); // consume opener
                let mut buf = String::new();
                let mut found_close = false;
                while let Some(&inner) = chars.peek() {
                    if inner == close {
                        chars.next(); // consume closer
                        found_close = true;
                        break;
                    }
                    buf.push(inner);
                    chars.next();
                }
                if !found_close {
                    // Unmatched opener — keep original characters
                    result.push(ch);
                    result.push_str(&buf);
                }
            }
            _ => {
                result.push(ch);
                chars.next();
            }
        }
    }

    // Collapse multiple spaces into one, then trim
    let mut prev_space = false;
    let collapsed: String = result
        .chars()
        .filter(|&c| {
            if c == ' ' {
                if prev_space {
                    return false;
                }
                prev_space = true;
            } else {
                prev_space = false;
            }
            true
        })
        .collect();
    collapsed.trim().to_string()
}

/// Station that transcribes audio chunks using a Whisper transcriber.
pub struct TranscriberStation {
    transcriber: Arc<dyn Transcriber>,
    verbose: bool,
    warned_backpressure: bool,
    hallucination_filters: Vec<String>,
}

impl TranscriberStation {
    /// Creates a new transcriber station.
    pub fn new(transcriber: Arc<dyn Transcriber>) -> Self {
        Self {
            transcriber,
            verbose: false,
            warned_backpressure: false,
            hallucination_filters: Vec::new(),
        }
    }

    /// Configure whether to enable diagnostic output to stderr.
    ///
    /// When verbose is true, diagnostic info is logged during transcription.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set hallucination filter phrases (pre-lowercased for O(1) runtime comparison).
    pub fn with_hallucination_filters(mut self, filters: Vec<String>) -> Self {
        self.hallucination_filters = filters.into_iter().map(|f| f.to_lowercase()).collect();
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

        let start = Instant::now();
        let chunk_duration_ms = chunk.duration_ms;

        // Attempt transcription
        let result = self
            .transcriber
            .transcribe(&chunk.samples)
            .map_err(|e| StationError::Recoverable(format!("Transcription failed: {}", e)))?;

        // Backpressure detection: warn once if transcription is slower than real-time
        if !self.warned_backpressure {
            let elapsed_ms = start.elapsed().as_millis() as u32;
            if elapsed_ms > chunk_duration_ms {
                self.warned_backpressure = true;
                eprintln_clear(&format!(
                    "voicsh: transcription slower than real-time ({elapsed_ms}ms for {chunk_duration_ms}ms of audio)"
                ));
                if cfg!(feature = "benchmark") {
                    eprintln!(
                        "  Run 'voicsh benchmark' to find the right model for your hardware."
                    );
                } else {
                    eprintln!("  Build with benchmark support to find the right model:");
                    eprintln!("    cargo build --release --features benchmark");
                }
                eprintln!(
                    "  Consider a smaller model (--model tiny.en) or enable GPU acceleration."
                );
                eprintln!("  To tolerate slower transcription, increase the buffer: --buffer 30s");
            }
        }

        // Clean Whisper markers
        let cleaned_text = clean_transcription(&result.text);

        // Skip empty results
        if cleaned_text.is_empty() {
            return Ok(None);
        }

        // Filter hallucinated phrases (exact match, case-insensitive)
        if !self.hallucination_filters.is_empty() {
            let lower = cleaned_text.to_lowercase();
            if self.hallucination_filters.iter().any(|f| f == &lower) {
                return Ok(None);
            }
        }

        // Return transcribed text with timing information from chunk (if available)
        Ok(Some(TranscribedText::with_timing(
            cleaned_text,
            chunk.timing,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::MockTranscriber;
    use std::time::Instant;

    #[test]
    fn test_successful_transcription() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Hello world"));

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
        let transcriber = Arc::new(MockTranscriber::new("mock").with_failure());

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3, 4, 5], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_err());
        match result {
            Err(StationError::Recoverable(msg)) => {
                assert!(msg.contains("Transcription failed"));
                assert!(msg.contains("mock transcription failure"));
            }
            _ => panic!("Expected Recoverable error"),
        }
    }

    #[test]
    fn test_whisper_marker_filtering() {
        let transcriber = Arc::new(
            MockTranscriber::new("mock")
                .with_response("Hello [BLANK_AUDIO] world [INAUDIBLE] test"),
        );

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert_eq!(text.text, "Hello world test");
    }

    #[test]
    fn test_multiple_markers_filtered() {
        let transcriber = Arc::new(
            MockTranscriber::new("mock")
                .with_response("[MUSIC] [APPLAUSE] Speech here [LAUGHTER] more speech [noise]"),
        );

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert_eq!(text.text, "Speech here more speech");
    }

    #[test]
    fn test_empty_result_returns_none() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response(""));

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_whitespace_only_returns_none() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("   \n\t  "));

        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_markers_only_returns_none() {
        let transcriber = Arc::new(
            MockTranscriber::new("mock").with_response("[BLANK_AUDIO] [INAUDIBLE] [silence]"),
        );

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
        assert_eq!(result, "text more");
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
    fn test_clean_transcription_german_annotations() {
        assert_eq!(clean_transcription("[Musik]"), "");
        assert_eq!(clean_transcription("*Klappern*"), "");
        assert_eq!(clean_transcription("[Lautes Klicken]"), "");
        assert_eq!(clean_transcription("[Lautes Lachen]"), "");
        assert_eq!(clean_transcription("*Klingeln*"), "");
    }

    #[test]
    fn test_clean_transcription_mixed_speech_and_annotations() {
        assert_eq!(clean_transcription("Hello [Musik] world"), "Hello world");
        assert_eq!(
            clean_transcription("Start *Klappern* middle (inaudible) end"),
            "Start middle end"
        );
    }

    #[test]
    fn test_clean_transcription_empty_annotations() {
        assert_eq!(clean_transcription("text [] more"), "text more");
        assert_eq!(clean_transcription("text ** more"), "text more");
        assert_eq!(clean_transcription("text () more"), "text more");
    }

    #[test]
    fn test_clean_transcription_unmatched_delimiters_pass_through() {
        assert_eq!(clean_transcription("price is 5["), "price is 5[");
        assert_eq!(clean_transcription("note (incomplete"), "note (incomplete");
        assert_eq!(
            clean_transcription("a * single asterisk"),
            "a * single asterisk"
        );
    }

    #[test]
    fn test_clean_transcription_collapses_multiple_spaces() {
        assert_eq!(clean_transcription("word [x] [y] [z] end"), "word end");
    }

    #[test]
    fn test_station_name() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response(""));

        let station = TranscriberStation::new(transcriber);
        assert_eq!(station.name(), "transcriber");
    }

    #[test]
    fn test_timestamp_is_current() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Test text"));

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

    // ── Backpressure detection tests ─────────────────────────────────────

    #[test]
    fn test_backpressure_detected_for_slow_transcription() {
        // Transcription takes 20ms but chunk is only 5ms of audio → backpressure.
        let transcriber = Arc::new(
            MockTranscriber::new("mock")
                .with_response("Hello")
                .with_delay(std::time::Duration::from_millis(20)),
        );
        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 5, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap().text, "Hello");
        assert!(
            station.warned_backpressure,
            "Should detect backpressure when transcription exceeds chunk duration"
        );
    }

    #[test]
    fn test_no_backpressure_for_fast_transcription() {
        // Chunk represents 100s of audio, instant mock → no backpressure.
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Hello"));
        let mut station = TranscriberStation::new(transcriber);

        let chunk = AudioChunk::new(vec![1, 2, 3], 100_000, 1);
        let result = station.process(chunk);

        assert!(result.is_ok());
        assert_eq!(result.unwrap().unwrap().text, "Hello");
        assert!(
            !station.warned_backpressure,
            "Should not warn when transcription is faster than real-time"
        );
    }

    #[test]
    fn test_backpressure_warning_fires_only_once() {
        let transcriber = Arc::new(
            MockTranscriber::new("mock")
                .with_response("Hello")
                .with_delay(std::time::Duration::from_millis(20)),
        );
        let mut station = TranscriberStation::new(transcriber);

        // First slow chunk triggers warning
        let chunk1 = AudioChunk::new(vec![1, 2, 3], 5, 1);
        let _ = station.process(chunk1);
        assert!(station.warned_backpressure);

        // Second slow chunk: warned_backpressure stays true, no second warning
        let chunk2 = AudioChunk::new(vec![1, 2, 3], 5, 2);
        let result = station.process(chunk2);
        assert!(result.is_ok());
        assert!(station.warned_backpressure);
    }

    #[test]
    fn test_backpressure_initial_state() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Hello"));
        let station = TranscriberStation::new(transcriber);
        assert!(
            !station.warned_backpressure,
            "Should start with no backpressure warning"
        );
    }

    // ── Hallucination filter tests ──────────────────────────────────────

    #[test]
    fn test_hallucination_filter_discards_match() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Thank you."));
        let mut station = TranscriberStation::new(transcriber)
            .with_hallucination_filters(vec!["Thank you.".to_string()]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(result.is_none(), "Hallucinated phrase should be discarded");
    }

    #[test]
    fn test_hallucination_filter_case_insensitive() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("THANK YOU."));
        let mut station = TranscriberStation::new(transcriber)
            .with_hallucination_filters(vec!["Thank you.".to_string()]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(result.is_none(), "Filter should be case-insensitive");
    }

    #[test]
    fn test_hallucination_filter_allows_non_match() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Hello world"));
        let mut station = TranscriberStation::new(transcriber)
            .with_hallucination_filters(vec!["Thank you.".to_string()]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(result.is_some(), "Non-matching text should pass through");
        assert_eq!(result.unwrap().text, "Hello world");
    }

    #[test]
    fn test_hallucination_filter_partial_match_passes() {
        let transcriber =
            Arc::new(MockTranscriber::new("mock").with_response("Thank you for coming"));
        let mut station = TranscriberStation::new(transcriber)
            .with_hallucination_filters(vec!["Thank you.".to_string()]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(
            result.is_some(),
            "Partial match should pass through (exact match only)"
        );
        assert_eq!(result.unwrap().text, "Thank you for coming");
    }

    #[test]
    fn test_hallucination_filter_empty_list_passes() {
        let transcriber = Arc::new(MockTranscriber::new("mock").with_response("Thank you."));
        let mut station = TranscriberStation::new(transcriber).with_hallucination_filters(vec![]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(result.is_some(), "Empty filter list should pass everything");
        assert_eq!(result.unwrap().text, "Thank you.");
    }

    #[test]
    fn test_hallucination_filter_after_annotation_removal() {
        // "[MUSIC] Thank you." → cleaned to "Thank you." → filtered
        let transcriber =
            Arc::new(MockTranscriber::new("mock").with_response("[MUSIC] Thank you."));
        let mut station = TranscriberStation::new(transcriber)
            .with_hallucination_filters(vec!["Thank you.".to_string()]);
        let chunk = AudioChunk::new(vec![1, 2, 3], 100, 1);
        let result = station.process(chunk).unwrap();
        assert!(
            result.is_none(),
            "After annotation removal, remaining hallucination should be filtered"
        );
    }
}
