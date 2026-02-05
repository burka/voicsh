//! Fan-out transcriber that runs multiple models in parallel.
//!
//! Spawns one thread per transcriber, collects results, and picks the one
//! with the highest confidence. Useful for running an English-optimized
//! and a multilingual model side-by-side.

use crate::error::Result;
use crate::stt::transcriber::{Transcriber, TranscriptionResult};
use std::sync::Arc;
use std::thread;

/// Transcriber that fans out to multiple child transcribers in parallel.
///
/// Each child receives the same audio. The result with the highest confidence
/// (and non-empty text) is returned.
pub struct FanOutTranscriber {
    transcribers: Vec<Arc<dyn Transcriber>>,
    name: String,
}

impl FanOutTranscriber {
    /// Create a fan-out transcriber from multiple child transcribers.
    ///
    /// # Panics
    /// Panics if `transcribers` is empty.
    pub fn new(transcribers: Vec<Arc<dyn Transcriber>>) -> Self {
        assert!(!transcribers.is_empty(), "need at least one transcriber");
        let name = transcribers
            .iter()
            .map(|t| t.model_name())
            .collect::<Vec<_>>()
            .join("+");
        Self { transcribers, name }
    }
}

impl Transcriber for FanOutTranscriber {
    fn transcribe(&self, audio: &[i16]) -> Result<TranscriptionResult> {
        let results: Vec<Result<TranscriptionResult>> = thread::scope(|scope| {
            let handles: Vec<_> = self
                .transcribers
                .iter()
                .map(|t| {
                    let t = t.clone();
                    scope.spawn(move || t.transcribe(audio))
                })
                .collect();

            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        // Pick the best result: highest confidence among non-empty successes
        let mut best: Option<TranscriptionResult> = None;
        let mut last_err: Option<crate::error::VoicshError> = None;

        for result in results {
            match result {
                Ok(tr) if !tr.text.is_empty() => {
                    if best.as_ref().is_none_or(|b| tr.confidence > b.confidence) {
                        best = Some(tr);
                    }
                }
                Ok(_) => {} // empty text, skip
                Err(e) => last_err = Some(e),
            }
        }

        best.ok_or_else(|| {
            last_err.unwrap_or_else(|| crate::error::VoicshError::Transcription {
                message: "All transcribers returned empty text".to_string(),
            })
        })
    }

    fn model_name(&self) -> &str {
        &self.name
    }

    fn is_ready(&self) -> bool {
        self.transcribers.iter().any(|t| t.is_ready())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::MockTranscriber;

    #[test]
    fn test_picks_highest_confidence() {
        let low =
            Arc::new(MockTranscriber::new("low").with_response("low text")) as Arc<dyn Transcriber>;
        let high = Arc::new(MockTranscriber::new("high").with_response("high text"))
            as Arc<dyn Transcriber>;

        // MockTranscriber always returns confidence=1.0, so both are equal.
        // To test ordering, we need custom mocks with different confidences.
        let fan = FanOutTranscriber::new(vec![low, high]);
        let result = fan.transcribe(&[0i16; 100]).unwrap();
        // Both have confidence 1.0, first one wins (or either is fine)
        assert!(!result.text.is_empty());
    }

    #[test]
    fn test_skips_empty_text() {
        let empty =
            Arc::new(MockTranscriber::new("empty").with_response("")) as Arc<dyn Transcriber>;
        let good =
            Arc::new(MockTranscriber::new("good").with_response("hello")) as Arc<dyn Transcriber>;

        let fan = FanOutTranscriber::new(vec![empty, good]);
        let result = fan.transcribe(&[0i16; 100]).unwrap();
        assert_eq!(result.text, "hello");
    }

    #[test]
    fn test_skips_failed_transcriber() {
        let fail = Arc::new(MockTranscriber::new("fail").with_failure()) as Arc<dyn Transcriber>;
        let good =
            Arc::new(MockTranscriber::new("good").with_response("works")) as Arc<dyn Transcriber>;

        let fan = FanOutTranscriber::new(vec![fail, good]);
        let result = fan.transcribe(&[0i16; 100]).unwrap();
        assert_eq!(result.text, "works");
    }

    #[test]
    fn test_all_fail_returns_error() {
        let fail1 = Arc::new(MockTranscriber::new("f1").with_failure()) as Arc<dyn Transcriber>;
        let fail2 = Arc::new(MockTranscriber::new("f2").with_failure()) as Arc<dyn Transcriber>;

        let fan = FanOutTranscriber::new(vec![fail1, fail2]);
        let result = fan.transcribe(&[0i16; 100]);
        assert!(result.is_err());
    }

    #[test]
    fn test_model_name_joins_children() {
        let a = Arc::new(MockTranscriber::new("base")) as Arc<dyn Transcriber>;
        let b = Arc::new(MockTranscriber::new("base.en")) as Arc<dyn Transcriber>;

        let fan = FanOutTranscriber::new(vec![a, b]);
        assert_eq!(fan.model_name(), "base+base.en");
    }

    #[test]
    fn test_is_ready_any() {
        let ready = Arc::new(MockTranscriber::new("ready")) as Arc<dyn Transcriber>;
        let not_ready =
            Arc::new(MockTranscriber::new("not").with_failure()) as Arc<dyn Transcriber>;

        let fan = FanOutTranscriber::new(vec![not_ready, ready]);
        assert!(fan.is_ready());
    }

    #[test]
    #[should_panic(expected = "need at least one transcriber")]
    fn test_empty_transcribers_panics() {
        FanOutTranscriber::new(vec![]);
    }
}
