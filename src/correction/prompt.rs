//! Generic utilities for correction pipeline.

use crate::stt::transcriber::TokenProbability;

/// Check whether any tokens need correction (below threshold).
///
/// Returns false if `tokens` is empty or all tokens are at/above threshold.
pub fn needs_correction(tokens: &[TokenProbability], threshold: f32) -> bool {
    !tokens.is_empty() && tokens.iter().any(|tp| tp.probability < threshold)
}

/// Check whether enough tokens need correction (proportional threshold).
///
/// Returns true only if at least `min_ratio` of tokens are below the
/// confidence threshold. This prevents correcting mostly-correct text
/// where T5 would likely corrupt good output.
pub fn needs_correction_proportional(
    tokens: &[TokenProbability],
    threshold: f32,
    min_ratio: f32,
) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let low_count = tokens
        .iter()
        .filter(|tp| tp.probability < threshold)
        .count();
    let ratio = low_count as f32 / tokens.len() as f32;
    ratio >= min_ratio
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
/// Returns true for:
/// - Empty or "auto" (language not yet determined)
/// - "en" (always correctable via T5 or passthrough)
/// - Languages in the SymSpell whitelist that have a dictionary
///
/// Languages with a dictionary but NOT in the whitelist are skipped
/// because SymSpell lowercases output, which destroys case-sensitive languages.
pub fn should_correct_language(language: &str, symspell_whitelist: &[String]) -> bool {
    if language.is_empty() || language == "auto" || language == "en" {
        return true;
    }
    let whitelisted =
        symspell_whitelist.is_empty() || symspell_whitelist.iter().any(|l| l == language);
    whitelisted && crate::dictionary::has_dictionary(language)
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
    fn needs_correction_proportional_empty_returns_false() {
        assert!(!needs_correction_proportional(&[], 0.7, 0.4));
    }

    #[test]
    fn needs_correction_proportional_all_high_returns_false() {
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
        assert!(!needs_correction_proportional(&tokens, 0.7, 0.4));
    }

    #[test]
    fn needs_correction_proportional_50_percent_low_returns_true() {
        let tokens = vec![
            TokenProbability {
                token: "hello".into(),
                probability: 0.50,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.85,
            },
        ];
        assert!(needs_correction_proportional(&tokens, 0.7, 0.4));
        assert!(!needs_correction_proportional(&tokens, 0.7, 0.6));
    }

    #[test]
    fn needs_correction_proportional_25_percent_low_returns_false_if_min_is_40() {
        let tokens = vec![
            TokenProbability {
                token: "hello".into(),
                probability: 0.50,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.85,
            },
            TokenProbability {
                token: " test".into(),
                probability: 0.90,
            },
            TokenProbability {
                token: " more".into(),
                probability: 0.95,
            },
        ];
        assert!(!needs_correction_proportional(&tokens, 0.7, 0.4));
        assert!(needs_correction_proportional(&tokens, 0.7, 0.2));
    }

    #[test]
    fn needs_correction_proportional_at_threshold_returns_true() {
        let tokens = vec![
            TokenProbability {
                token: "hello".into(),
                probability: 0.65,
            },
            TokenProbability {
                token: " world".into(),
                probability: 0.85,
            },
        ];
        assert!(needs_correction_proportional(&tokens, 0.7, 0.4));
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

    /// Default SymSpell whitelist from config.
    fn default_whitelist() -> Vec<String> {
        vec![
            "he".to_string(),
            "ar".to_string(),
            "zh".to_string(),
            "ja".to_string(),
            "ko".to_string(),
        ]
    }

    #[test]
    fn test_should_correct_language_en() {
        let wl = default_whitelist();
        assert!(
            should_correct_language("en", &wl),
            "English always correctable"
        );
    }

    #[test]
    fn test_should_correct_language_auto() {
        let wl = default_whitelist();
        assert!(
            should_correct_language("auto", &wl),
            "Auto always correctable"
        );
    }

    #[test]
    fn test_should_correct_language_empty() {
        let wl = default_whitelist();
        assert!(
            should_correct_language("", &wl),
            "Empty language always correctable"
        );
    }

    #[test]
    fn test_should_correct_language_hebrew_whitelisted() {
        let wl = default_whitelist();
        assert!(
            should_correct_language("he", &wl),
            "Hebrew is whitelisted and has dictionary"
        );
    }

    #[test]
    fn test_should_correct_language_german_not_whitelisted() {
        let wl = default_whitelist();
        assert!(
            !should_correct_language("de", &wl),
            "German has dictionary but is NOT whitelisted (SymSpell would destroy casing)"
        );
    }

    #[test]
    fn test_should_correct_language_french_not_whitelisted() {
        let wl = default_whitelist();
        assert!(
            !should_correct_language("fr", &wl),
            "French has dictionary but is NOT whitelisted"
        );
    }

    #[test]
    fn test_should_correct_language_japanese_no_dictionary() {
        let wl = default_whitelist();
        assert!(
            !should_correct_language("ja", &wl),
            "Japanese is whitelisted but has no dictionary"
        );
    }

    #[test]
    fn test_should_correct_language_empty_whitelist_allows_all_with_dict() {
        let empty_wl: Vec<String> = vec![];
        assert!(
            should_correct_language("de", &empty_wl),
            "Empty whitelist = all languages with dictionary allowed"
        );
        assert!(
            should_correct_language("fr", &empty_wl),
            "Empty whitelist = all languages with dictionary allowed"
        );
    }

    #[test]
    fn test_should_correct_language_unknown_rejected() {
        let wl = default_whitelist();
        assert!(
            !should_correct_language("xx", &wl),
            "Unknown language has no dictionary"
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
