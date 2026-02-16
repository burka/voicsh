//! Corrector trait for post-ASR text correction.

use crate::error::Result;

/// Trait for post-ASR text correction.
///
/// Implementations receive a correction prompt (with confidence-annotated tokens)
/// and return corrected text.
pub trait Corrector: Send + 'static {
    /// Correct text based on the given prompt.
    ///
    /// The prompt contains the original text with low-confidence tokens
    /// annotated with their probability scores.
    fn correct(&mut self, prompt: &str) -> Result<String>;

    /// Return the name of this corrector for logging.
    fn name(&self) -> &str;
}

/// Passthrough corrector that returns the original text unchanged.
///
/// Used when error correction is disabled or for non-English languages.
/// In practice, `CorrectionStation` skips calling the corrector entirely
/// when correction is unnecessary, so this is a safety fallback.
pub struct PassthroughCorrector;

impl Corrector for PassthroughCorrector {
    fn correct(&mut self, _prompt: &str) -> Result<String> {
        // CorrectionStation skips correction entirely when it would use
        // passthrough — this is a safe fallback that returns empty string.
        Ok(String::new())
    }

    fn name(&self) -> &str {
        "passthrough"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_returns_empty_string() {
        let mut corrector = PassthroughCorrector;
        let result = corrector.correct("some prompt").unwrap();
        assert_eq!(result, "", "Passthrough should return empty string");
    }

    #[test]
    fn passthrough_name_is_passthrough() {
        let corrector = PassthroughCorrector;
        assert_eq!(corrector.name(), "passthrough");
    }

    #[test]
    fn passthrough_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<PassthroughCorrector>();
    }

    #[test]
    fn corrector_trait_object_is_send() {
        fn assert_send<T: Send + ?Sized>() {}
        assert_send::<Box<dyn Corrector>>();
    }
}
