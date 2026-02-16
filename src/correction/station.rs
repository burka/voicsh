//! CorrectionStation: post-ASR error correction pipeline stage.

use crate::config::ErrorCorrectionConfig;
use crate::correction::corrector::Corrector;
use crate::correction::prompt;
use crate::ipc::protocol::TextOrigin;
use crate::pipeline::error::StationError;
use crate::pipeline::station::Station;
use crate::pipeline::types::TranscribedText;

/// Pipeline station that applies post-ASR error correction.
///
/// Only corrects English text with low-confidence tokens.
/// Falls back to raw text on timeout or error.
pub struct CorrectionStation {
    corrector: Box<dyn Corrector>,
    config: ErrorCorrectionConfig,
}

impl CorrectionStation {
    /// Create a new CorrectionStation with the given corrector and config.
    pub fn new(corrector: Box<dyn Corrector>, config: ErrorCorrectionConfig) -> Self {
        Self { corrector, config }
    }
}

impl Station for CorrectionStation {
    type Input = TranscribedText;
    type Output = TranscribedText;

    fn process(&mut self, mut input: Self::Input) -> Result<Option<Self::Output>, StationError> {
        // Skip if correction is disabled
        if !self.config.enabled {
            return Ok(Some(input));
        }

        // Skip for non-English languages
        if !prompt::should_correct_language(&input.language) {
            return Ok(Some(input));
        }

        // Build correction prompt — returns None if all tokens are high confidence
        let correction_prompt = match prompt::build_correction_prompt(
            &input.token_probabilities,
            self.config.confidence_threshold,
        ) {
            Some(p) => p,
            None => return Ok(Some(input)),
        };

        // Apply correction — fall back to raw text on error
        match self.corrector.correct(&correction_prompt) {
            Ok(corrected) if !corrected.is_empty() => {
                if corrected != input.text {
                    input.raw_text = Some(input.text.clone());
                    input.text = corrected;
                    input.text_origin = TextOrigin::Corrected;
                }
            }
            Ok(_) => {
                // Empty correction result — keep raw text
            }
            Err(e) => {
                eprintln!(
                    "voicsh: {} correction failed: {}, using raw text",
                    self.corrector.name(),
                    e
                );
            }
        }

        Ok(Some(input))
    }

    fn name(&self) -> &'static str {
        "Correction"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ErrorCorrectionConfig;
    use crate::error::VoicshError;
    use crate::pipeline::types::{SinkEvent, TranscribedText};
    use crate::stt::transcriber::TokenProbability;
    use std::time::Instant;

    /// Corrector that returns a fixed response or fails on demand.
    struct FixedCorrectorForTest {
        response: String,
        should_fail: bool,
    }

    impl Corrector for FixedCorrectorForTest {
        fn correct(&mut self, _prompt: &str) -> crate::error::Result<String> {
            if self.should_fail {
                Err(VoicshError::Other("correction failed".into()))
            } else {
                Ok(self.response.clone())
            }
        }
        fn name(&self) -> &str {
            "fixed-corrector"
        }
    }

    fn enabled_config(threshold: f32) -> ErrorCorrectionConfig {
        ErrorCorrectionConfig {
            enabled: true,
            model: "flan-t5-small".to_string(),
            confidence_threshold: threshold,
            timeout_ms: 2000,
        }
    }

    fn disabled_config() -> ErrorCorrectionConfig {
        ErrorCorrectionConfig {
            enabled: false,
            ..enabled_config(0.7)
        }
    }

    fn make_input(text: &str, language: &str, tokens: Vec<TokenProbability>) -> TranscribedText {
        TranscribedText {
            text: text.to_string(),
            language: language.to_string(),
            confidence: 0.9,
            timestamp: Instant::now(),
            timing: None,
            events: vec![SinkEvent::Text("test".into())],
            token_probabilities: tokens,
            raw_text: None,
            text_origin: TextOrigin::default(),
        }
    }

    fn low_confidence_tokens() -> Vec<TokenProbability> {
        vec![
            TokenProbability {
                token: "the".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " quik".into(),
                probability: 0.30,
            },
            TokenProbability {
                token: " brown".into(),
                probability: 0.92,
            },
        ]
    }

    fn high_confidence_tokens() -> Vec<TokenProbability> {
        vec![
            TokenProbability {
                token: "hello".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.88,
            },
        ]
    }

    #[test]
    fn disabled_config_passes_through_unchanged() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "should not appear".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, disabled_config());
        let input = make_input("original text", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "original text");
    }

    #[test]
    fn non_english_language_passes_through() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "should not appear".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("original text", "de", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "original text");
    }

    #[test]
    fn high_confidence_tokens_pass_through() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "should not appear".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("hello world", "en", high_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn low_confidence_tokens_trigger_correction() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "the quick brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("the quik brown", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "the quick brown");
    }

    #[test]
    fn corrector_error_falls_back_to_raw_text() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: String::new(),
            should_fail: true,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("raw text", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "raw text");
    }

    #[test]
    fn empty_correction_result_keeps_raw_text() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: String::new(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("raw text", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "raw text");
    }

    #[test]
    fn preserves_timing_events_language_confidence() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "corrected".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let timestamp = Instant::now();
        let input = TranscribedText {
            text: "original".to_string(),
            language: "en".to_string(),
            confidence: 0.85,
            timestamp,
            timing: None,
            events: vec![SinkEvent::Text("event1".into())],
            token_probabilities: low_confidence_tokens(),
            raw_text: None,
            text_origin: TextOrigin::default(),
        };
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "corrected");
        assert_eq!(result.language, "en");
        assert_eq!(result.confidence, 0.85);
        assert_eq!(result.timestamp, timestamp);
        assert_eq!(result.events, vec![SinkEvent::Text("event1".into())]);
    }

    #[test]
    fn empty_token_probabilities_pass_through() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "should not appear".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("no tokens", "en", vec![]);
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "no tokens");
    }

    #[test]
    fn auto_language_triggers_correction() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "corrected".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("original", "auto", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "corrected");
    }

    #[test]
    fn empty_language_triggers_correction() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "corrected".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("original", "", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "corrected");
    }

    #[test]
    fn station_name_is_correction() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: String::new(),
            should_fail: false,
        });
        let station = CorrectionStation::new(corrector, disabled_config());
        assert_eq!(station.name(), "Correction");
    }

    #[test]
    fn correction_sets_raw_text_and_origin() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "the quick brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("the quik brown", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "the quick brown");
        assert_eq!(result.raw_text, Some("the quik brown".to_string()));
        assert_eq!(result.text_origin, TextOrigin::Corrected);
    }

    #[test]
    fn no_change_preserves_default_origin() {
        // When corrector returns identical text, provenance should stay default
        let corrector = Box::new(FixedCorrectorForTest {
            response: "the quik brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("the quik brown", "en", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "the quik brown");
        assert_eq!(result.raw_text, None);
        assert_eq!(result.text_origin, TextOrigin::Transcription);
    }
}
