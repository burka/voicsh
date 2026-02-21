//! Generic utilities for correction pipeline.

use crate::stt::transcriber::TokenProbability;

/// Check whether any tokens need correction (below threshold).
///
/// Returns false if `tokens` is empty or all tokens are at/above threshold.
pub fn needs_correction(tokens: &[TokenProbability], threshold: f32) -> bool {
    !tokens.is_empty() && tokens.iter().any(|tp| tp.probability < threshold)
}

/// Reconstruct raw text from token probabilities (trimmed).
pub fn extract_raw_text(tokens: &[TokenProbability]) -> String {
    tokens
        .iter()
        .map(|tp| tp.token.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Levenshtein edit distance between two strings (character-level).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Check whether correction should be attempted for the given language.
///
/// Allows languages that have a SymSpell dictionary available,
/// plus empty string and "auto" which are treated as potentially
/// correctable.
pub fn should_correct_language(language: &str) -> bool {
    language.is_empty() || language == "auto" || crate::dictionary::has_dictionary(language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::TokenProbability;

    #[test]
    fn needs_correction_empty_returns_false() {
        assert!(!needs_correction(&[], 0.7));
    }

    #[test]
    fn needs_correction_all_high_returns_false() {
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
        assert!(!needs_correction(&tokens, 0.7));
    }

    #[test]
    fn needs_correction_some_low_returns_true() {
        let tokens = vec![
            TokenProbability {
                token: "the".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " quik".into(),
                probability: 0.30,
            },
        ];
        assert!(needs_correction(&tokens, 0.7));
    }

    #[test]
    fn needs_correction_at_threshold_returns_false() {
        let tokens = vec![TokenProbability {
            token: "exact".into(),
            probability: 0.70,
        }];
        assert!(!needs_correction(&tokens, 0.70));
    }

    #[test]
    fn extract_raw_text_concatenates_tokens() {
        let tokens = vec![
            TokenProbability {
                token: "the".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " quick".into(),
                probability: 0.88,
            },
            TokenProbability {
                token: " brown".into(),
                probability: 0.92,
            },
        ];
        assert_eq!(extract_raw_text(&tokens), "the quick brown");
    }

    #[test]
    fn extract_raw_text_trims_leading_whitespace() {
        let tokens = vec![
            TokenProbability {
                token: " hello".into(),
                probability: 0.95,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.88,
            },
        ];
        assert_eq!(extract_raw_text(&tokens), "hello world");
    }

    #[test]
    fn extract_raw_text_empty_returns_empty() {
        assert_eq!(extract_raw_text(&[]), "");
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
            should_correct_language("de"),
            "German should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_french() {
        assert!(
            should_correct_language("fr"),
            "French should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_spanish() {
        assert!(
            should_correct_language("es"),
            "Spanish should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_italian() {
        assert!(
            should_correct_language("it"),
            "Italian should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_russian() {
        assert!(
            should_correct_language("ru"),
            "Russian should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_hebrew() {
        assert!(
            should_correct_language("he"),
            "Hebrew should be corrected (dictionary available)"
        );
    }

    #[test]
    fn test_should_correct_language_japanese() {
        assert!(
            !should_correct_language("ja"),
            "Japanese should NOT be corrected (no dictionary)"
        );
    }

    #[test]
    fn test_should_correct_language_chinese() {
        assert!(
            !should_correct_language("zh"),
            "Chinese should NOT be corrected (no dictionary)"
        );
    }

    #[test]
    fn test_should_correct_language_korean() {
        assert!(
            !should_correct_language("ko"),
            "Korean should NOT be corrected (no dictionary)"
        );
    }

    #[test]
    fn test_edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_edit_distance_one_char() {
        assert_eq!(edit_distance("quik", "quick"), 1);
    }

    #[test]
    fn test_edit_distance_completely_different() {
        let d = edit_distance("hello world", "completely different");
        assert!(d > 10, "Should be large: {d}");
    }
}
