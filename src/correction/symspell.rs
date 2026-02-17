//! SymSpell-based error corrector using dictionary lookup.
//!
//! Sub-millisecond correction via compound word splitting and
//! edit-distance matching against a built-in English frequency dictionary.

use crate::correction::corrector::Corrector;
use crate::error::Result;
use symspell::{AsciiStringStrategy, SymSpell};

/// Embedded English frequency dictionary (82,765 entries, ~1.3 MB).
const DICTIONARY: &str = include_str!("../../data/frequency_dictionary_en_82_765.txt");

/// SymSpell corrector that uses dictionary-based compound word correction.
pub struct SymSpellCorrector {
    symspell: SymSpell<AsciiStringStrategy>,
}

impl SymSpellCorrector {
    /// Create a new SymSpellCorrector with the embedded dictionary.
    pub fn new() -> Result<Self> {
        let mut symspell: SymSpell<AsciiStringStrategy> = SymSpell::default();

        for line in DICTIONARY.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2
                && let Ok(freq) = parts[1].parse::<i64>()
            {
                symspell.load_dictionary_line(&format!("{} {}", parts[0], freq), 0, 1, " ");
            }
        }

        Ok(Self { symspell })
    }
}

impl Corrector for SymSpellCorrector {
    fn correct(&mut self, text: &str) -> Result<String> {
        let suggestions = self.symspell.lookup_compound(text, 2);
        if let Some(suggestion) = suggestions.first() {
            Ok(suggestion.term.clone())
        } else {
            Ok(text.to_string())
        }
    }

    fn name(&self) -> &str {
        "symspell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symspell_corrector_loads_dictionary() {
        let corrector = SymSpellCorrector::new();
        assert!(corrector.is_ok(), "Should load dictionary without error");
    }

    #[test]
    fn symspell_corrects_simple_typo() {
        let mut corrector = SymSpellCorrector::new().unwrap();
        let result = corrector.correct("the quik brown fox").unwrap();
        assert_ne!(result, "the quik brown fox", "Should correct 'quik'");
        assert!(
            result.contains("quick") || result.contains("the"),
            "Result should be reasonable: {}",
            result
        );
    }

    #[test]
    fn symspell_passes_correct_text() {
        let mut corrector = SymSpellCorrector::new().unwrap();
        let result = corrector.correct("hello world").unwrap();
        assert_eq!(
            result, "hello world",
            "Correct text should pass through unchanged"
        );
    }

    #[test]
    fn symspell_handles_empty_string() {
        let mut corrector = SymSpellCorrector::new().unwrap();
        let result = corrector.correct("").unwrap();
        assert_eq!(result, "", "Empty string should return empty");
    }

    #[test]
    fn symspell_name_is_symspell() {
        let corrector = SymSpellCorrector::new().unwrap();
        assert_eq!(corrector.name(), "symspell");
    }

    #[test]
    fn symspell_corrector_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<SymSpellCorrector>();
    }
}
