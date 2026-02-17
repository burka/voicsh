//! CorrectionStation: post-ASR error correction pipeline stage.

use crate::config::ErrorCorrectionConfig;
use crate::correction::corrector::Corrector;
use crate::correction::prompt;
use crate::ipc::protocol::TextOrigin;
use crate::pipeline::error::StationError;
use crate::pipeline::station::Station;
use crate::pipeline::types::TranscribedText;
use std::sync::atomic::{AtomicBool, Ordering};

static LANGUAGE_SKIP_LOGGED: AtomicBool = AtomicBool::new(false);

/// Pipeline station that applies post-ASR error correction.
///
/// Only corrects languages with a SymSpell dictionary and low-confidence tokens.
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
        if !self.config.enabled {
            return Ok(Some(input));
        }

        if !prompt::should_correct_language(&input.language) {
            if !LANGUAGE_SKIP_LOGGED.swap(true, Ordering::Relaxed) {
                eprintln!("voicsh: correction skipped (language '{}')", input.language);
            }
            return Ok(Some(input));
        }

        if !prompt::needs_correction(&input.token_probabilities, self.config.confidence_threshold) {
            eprintln!(
                "voicsh: correction skipped (all tokens above confidence threshold {})",
                self.config.confidence_threshold
            );
            return Ok(Some(input));
        }

        let raw_text = prompt::extract_raw_text(&input.token_probabilities);

        match self.corrector.correct(&raw_text) {
            Ok(corrected) => {
                // Validate: non-empty, not too divergent
                if corrected.is_empty() {
                    return Ok(Some(input));
                }
                let max_len = input.text.len().max(corrected.len());
                if max_len > 5 {
                    let distance = prompt::edit_distance(&input.text, &corrected);
                    let change_ratio = distance as f64 / max_len as f64;
                    if change_ratio > 0.4 {
                        eprintln!(
                            "voicsh: correction rejected (too divergent: {:.0}% > 40%)",
                            change_ratio * 100.0
                        );
                        return Ok(Some(input));
                    }
                }
                if corrected != input.text {
                    input.raw_text = Some(input.text.clone());
                    input.text = corrected;
                    input.text_origin = TextOrigin::Corrected;
                }
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
            confidence_threshold: threshold,
            ..Default::default()
        }
    }

    fn disabled_config() -> ErrorCorrectionConfig {
        ErrorCorrectionConfig {
            enabled: false,
            ..Default::default()
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
    fn unsupported_language_passes_through() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "should not appear".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("original text", "ja", low_confidence_tokens());
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
            response: "the quick brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let timestamp = Instant::now();
        let input = TranscribedText {
            text: "the quik brown".to_string(),
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
        assert_eq!(result.text, "the quick brown");
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
            response: "the quick brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("the quik brown", "auto", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "the quick brown");
    }

    #[test]
    fn empty_language_triggers_correction() {
        let corrector = Box::new(FixedCorrectorForTest {
            response: "the quick brown".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input("the quik brown", "", low_confidence_tokens());
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "the quick brown");
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

    #[test]
    fn too_divergent_correction_rejected() {
        // Model returns something completely different (edit distance > 40%)
        let corrector = Box::new(FixedCorrectorForTest {
            response: "es scheint die T5 weiss ueber German".into(),
            should_fail: false,
        });
        let mut station = CorrectionStation::new(corrector, enabled_config(0.7));
        let input = make_input(
            "it seems the T5 knows about German",
            "en",
            low_confidence_tokens(),
        );
        let result = station.process(input).unwrap().unwrap();
        assert_eq!(result.text, "it seems the T5 knows about German");
        assert_eq!(result.raw_text, None);
    }
}
