//! Build correction prompts from transcription text and token probabilities.

use crate::stt::transcriber::TokenProbability;

/// Build a correction prompt with low-confidence tokens annotated.
///
/// Tokens with probability below `threshold` are marked: `word[0.34]`.
/// Returns `None` if no tokens are below threshold (no correction needed).
pub fn build_correction_prompt(
    token_probabilities: &[TokenProbability],
    threshold: f32,
) -> Option<String> {
    if token_probabilities.is_empty() {
        return None;
    }

    let needs_correction = token_probabilities
        .iter()
        .any(|tp| tp.probability < threshold);
    if !needs_correction {
        return None;
    }

    let mut annotated = String::new();
    for tp in token_probabilities {
        if tp.probability < threshold {
            annotated.push_str(&format!("{}[{:.2}]", tp.token, tp.probability));
        } else {
            annotated.push_str(&tp.token);
        }
    }

    Some(format!(
        "Fix errors in this speech transcript. Words marked with [score] have low ASR confidence:\n{}",
        annotated
    ))
}

/// Check whether correction should be attempted for the given language.
///
/// Only English is supported. Empty string and "auto" are treated as
/// potentially English and allowed through.
pub fn should_correct_language(language: &str) -> bool {
    language.is_empty() || language == "en" || language == "auto"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::TokenProbability;

    #[test]
    fn test_empty_probabilities_returns_none() {
        let result = build_correction_prompt(&[], 0.7);
        assert!(result.is_none(), "Empty token list should return None");
    }

    #[test]
    fn test_all_high_confidence_returns_none() {
        let tokens = vec![
            TokenProbability {
                token: "hello".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.88,
            },
        ];
        let result = build_correction_prompt(&tokens, 0.7);
        assert!(
            result.is_none(),
            "All tokens above threshold should return None"
        );
    }

    #[test]
    fn test_low_confidence_tokens_annotated() {
        let tokens = vec![
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
            TokenProbability {
                token: " foks".into(),
                probability: 0.25,
            },
        ];
        let result = build_correction_prompt(&tokens, 0.7);
        assert!(
            result.is_some(),
            "Should return prompt when tokens below threshold"
        );
        let prompt = result.unwrap();
        assert!(
            prompt.contains(" quik[0.30]"),
            "Low-confidence token should be annotated: {}",
            prompt
        );
        assert!(
            prompt.contains(" foks[0.25]"),
            "Low-confidence token should be annotated: {}",
            prompt
        );
        assert!(
            prompt.contains("the"),
            "High-confidence token should appear without annotation: {}",
            prompt
        );
        assert!(
            !prompt.contains("the["),
            "High-confidence token should NOT be annotated: {}",
            prompt
        );
        assert!(
            prompt.contains("Fix errors in this speech transcript"),
            "Should contain instruction prefix: {}",
            prompt
        );
    }

    #[test]
    fn test_single_low_confidence_token() {
        let tokens = vec![TokenProbability {
            token: "word".into(),
            probability: 0.40,
        }];
        let prompt = build_correction_prompt(&tokens, 0.7)
            .expect("Single low-confidence token should produce a prompt");
        assert!(
            prompt.contains("word[0.40]"),
            "Single low token should be annotated: {}",
            prompt
        );
    }

    #[test]
    fn test_threshold_boundary_equal_not_annotated() {
        let tokens = vec![TokenProbability {
            token: "exact".into(),
            probability: 0.70,
        }];
        let result = build_correction_prompt(&tokens, 0.70);
        assert!(
            result.is_none(),
            "Token at exact threshold should NOT trigger correction"
        );
    }

    #[test]
    fn test_threshold_boundary_just_below() {
        let tokens = vec![TokenProbability {
            token: "close".into(),
            probability: 0.699,
        }];
        let result = build_correction_prompt(&tokens, 0.70);
        assert!(
            result.is_some(),
            "Token just below threshold should trigger correction"
        );
    }

    #[test]
    fn test_should_correct_language_en() {
        assert!(should_correct_language("en"), "English should be corrected");
    }

    #[test]
    fn test_should_correct_language_auto() {
        assert!(should_correct_language("auto"), "Auto should be corrected");
    }

    #[test]
    fn test_should_correct_language_empty() {
        assert!(
            should_correct_language(""),
            "Empty language should be corrected"
        );
    }

    #[test]
    fn test_should_correct_language_german() {
        assert!(
            !should_correct_language("de"),
            "German should NOT be corrected"
        );
    }

    #[test]
    fn test_should_correct_language_french() {
        assert!(
            !should_correct_language("fr"),
            "French should NOT be corrected"
        );
    }

    #[test]
    fn test_should_correct_language_japanese() {
        assert!(
            !should_correct_language("ja"),
            "Japanese should NOT be corrected"
        );
    }
}
