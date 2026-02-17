//! SymSpell-based error corrector using dictionary lookup.
//!
//! Sub-millisecond correction via compound word splitting and
//! edit-distance matching against a frequency dictionary loaded at runtime.

use crate::correction::corrector::Corrector;
use crate::error::Result;
use std::path::Path;
use symspell::{SymSpell, UnicodeStringStrategy};

/// SymSpell corrector that uses dictionary-based compound word correction.
pub struct SymSpellCorrector {
    symspell: SymSpell<UnicodeStringStrategy>,
    language: String,
}

impl std::fmt::Debug for SymSpellCorrector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymSpellCorrector")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl SymSpellCorrector {
    /// Create a new SymSpellCorrector by loading a dictionary from file.
    ///
    /// The file should contain one entry per line: `word frequency`
    /// (whitespace-separated).
    pub fn from_file(path: &Path, language: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::VoicshError::Other(format!(
                "Failed to read dictionary '{}': {}",
                path.display(),
                e
            ))
        })?;

        let mut symspell: SymSpell<UnicodeStringStrategy> = SymSpell::default();

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2
                && let Ok(freq) = parts[1].parse::<i64>()
            {
                symspell.load_dictionary_line(&format!("{} {}", parts[0], freq), 0, 1, " ");
            }
        }

        Ok(Self {
            symspell,
            language: language.to_string(),
        })
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
        // Return a static reference for the common cases, dynamic for others
        match self.language.as_str() {
            "en" => "symspell-en",
            "de" => "symspell-de",
            "es" => "symspell-es",
            "fr" => "symspell-fr",
            "he" => "symspell-he",
            "it" => "symspell-it",
            "ru" => "symspell-ru",
            _ => "symspell",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Create a temporary dictionary file with test entries.
    fn create_test_dictionary() -> (tempfile::NamedTempFile, std::path::PathBuf) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "hello 1000000").unwrap();
        writeln!(file, "world 500000").unwrap();
        writeln!(file, "the 2000000").unwrap();
        writeln!(file, "quick 300000").unwrap();
        writeln!(file, "brown 200000").unwrap();
        writeln!(file, "fox 150000").unwrap();
        file.flush().unwrap();
        let path = file.path().to_path_buf();
        (file, path)
    }

    #[test]
    fn from_file_loads_dictionary() {
        let (_file, path) = create_test_dictionary();
        let corrector = SymSpellCorrector::from_file(&path, "en");
        assert!(corrector.is_ok(), "Should load dictionary without error");
    }

    #[test]
    fn from_file_nonexistent_returns_error() {
        let result = SymSpellCorrector::from_file(Path::new("/nonexistent/dict.txt"), "en");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to read dictionary"),
            "Error should mention reading failure: {}",
            err
        );
    }

    #[test]
    fn from_file_passes_correct_text() {
        let (_file, path) = create_test_dictionary();
        let mut corrector = SymSpellCorrector::from_file(&path, "en").unwrap();
        let result = corrector.correct("hello world").unwrap();
        assert_eq!(
            result, "hello world",
            "Correct text should pass through unchanged"
        );
    }

    #[test]
    fn from_file_handles_empty_string() {
        let (_file, path) = create_test_dictionary();
        let mut corrector = SymSpellCorrector::from_file(&path, "en").unwrap();
        let result = corrector.correct("").unwrap();
        assert_eq!(result, "", "Empty string should return empty");
    }

    #[test]
    fn name_includes_language() {
        let (_file, path) = create_test_dictionary();
        let corrector = SymSpellCorrector::from_file(&path, "en").unwrap();
        assert_eq!(corrector.name(), "symspell-en");

        let corrector = SymSpellCorrector::from_file(&path, "de").unwrap();
        assert_eq!(corrector.name(), "symspell-de");

        let corrector = SymSpellCorrector::from_file(&path, "fr").unwrap();
        assert_eq!(corrector.name(), "symspell-fr");
    }

    #[test]
    fn symspell_corrector_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<SymSpellCorrector>();
    }

    #[test]
    fn from_file_empty_dictionary() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let corrector = SymSpellCorrector::from_file(&path, "en");
        assert!(
            corrector.is_ok(),
            "Empty dictionary should load without error"
        );
    }

    #[test]
    fn from_file_malformed_lines_skipped() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "hello 1000000").unwrap();
        writeln!(file, "single_word_no_freq").unwrap();
        writeln!(file, "world notanumber").unwrap();
        writeln!(file, "good 500000").unwrap();
        file.flush().unwrap();
        let path = file.path().to_path_buf();
        let corrector = SymSpellCorrector::from_file(&path, "en");
        assert!(
            corrector.is_ok(),
            "Malformed lines should be skipped silently"
        );
    }
}
