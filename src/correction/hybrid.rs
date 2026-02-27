//! Hybrid corrector that dispatches correction based on language.
//!
//! Language dispatch strategy:
//! - English (en): Uses T5 (English-only model)
//! - Whitelisted (he, ar, zh, ja, ko): Uses SymSpell (no casing issues)
//! - Other languages: Passes through unchanged (neither T5 nor SymSpell is suitable)
//!
//! SymSpell lowercases all output, so it's only recommended for languages where
//! capitalization doesn't carry semantic meaning (Hebrew, Arabic, Chinese, Japanese, Korean).
//!
//! T5 is English-only and produces garbage on non-English text, so it's only used for English.

use crate::correction::corrector::Corrector;
use std::collections::HashMap;
use std::sync::Arc;

/// Hybrid corrector that holds both T5 and SymSpell backends.
///
/// The Corrector trait handles language dispatch, calling the
/// appropriate backend based on the detected language.
#[cfg(all(feature = "error-correction", feature = "symspell"))]
pub struct HybridCorrector {
    t5: Option<Box<dyn Corrector>>,
    symspell: HashMap<String, Box<dyn Corrector>>,
    symspell_whitelist: Arc<Vec<String>>,
    last_backend: Option<String>,
}

#[cfg(feature = "error-correction")]
#[cfg(not(feature = "symspell"))]
pub struct HybridCorrector {
    t5: Option<Box<dyn Corrector>>,
    #[allow(dead_code)]
    symspell_whitelist: Arc<Vec<String>>,
    last_backend: Option<String>,
}

#[cfg(feature = "symspell")]
#[cfg(not(feature = "error-correction"))]
pub struct HybridCorrector {
    symspell: HashMap<String, Box<dyn Corrector>>,
    symspell_whitelist: Arc<Vec<String>>,
    last_backend: Option<String>,
}

#[cfg(not(any(feature = "error-correction", feature = "symspell")))]
pub struct HybridCorrector {
    #[allow(dead_code)]
    symspell_whitelist: Arc<Vec<String>>,
    last_backend: Option<String>,
}

#[cfg(all(feature = "error-correction", feature = "symspell"))]
impl std::fmt::Debug for HybridCorrector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridCorrector")
            .field("t5", &self.t5.is_some())
            .field("symspell", &format_args!("[{} langs]", self.symspell.len()))
            .field(
                "symspell_whitelist",
                &format_args!("[{} langs]", self.symspell_whitelist.len()),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "error-correction")]
#[cfg(not(feature = "symspell"))]
impl std::fmt::Debug for HybridCorrector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridCorrector")
            .field("t5", &self.t5.is_some())
            .field(
                "symspell_whitelist",
                &format_args!("[{} langs]", self.symspell_whitelist.len()),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "symspell")]
#[cfg(not(feature = "error-correction"))]
impl std::fmt::Debug for HybridCorrector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridCorrector")
            .field("symspell", &format_args!("[{} langs]", self.symspell.len()))
            .field(
                "symspell_whitelist",
                &format_args!("[{} langs]", self.symspell_whitelist.len()),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(not(any(feature = "error-correction", feature = "symspell")))]
impl std::fmt::Debug for HybridCorrector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridCorrector").finish_non_exhaustive()
    }
}

#[cfg(all(feature = "error-correction", feature = "symspell"))]
impl HybridCorrector {
    pub fn new(
        t5: Option<Box<dyn Corrector>>,
        symspell: HashMap<String, Box<dyn Corrector>>,
        whitelist: Vec<String>,
    ) -> Self {
        Self {
            t5,
            symspell,
            symspell_whitelist: Arc::new(whitelist),
            last_backend: None,
        }
    }

    pub fn has_t5(&self) -> bool {
        self.t5.is_some()
    }

    pub fn symspell_count(&self) -> usize {
        self.symspell.len()
    }
}

#[cfg(feature = "error-correction")]
#[cfg(not(feature = "symspell"))]
impl HybridCorrector {
    pub fn new(t5: Option<Box<dyn Corrector>>) -> Self {
        Self {
            t5,
            symspell_whitelist: Arc::new(Vec::new()),
            last_backend: None,
        }
    }

    pub fn has_t5(&self) -> bool {
        self.t5.is_some()
    }
}

#[cfg(feature = "symspell")]
#[cfg(not(feature = "error-correction"))]
impl HybridCorrector {
    pub fn new(symspell: HashMap<String, Box<dyn Corrector>>, whitelist: Vec<String>) -> Self {
        Self {
            symspell,
            symspell_whitelist: Arc::new(whitelist),
            last_backend: None,
        }
    }

    pub fn symspell_count(&self) -> usize {
        self.symspell.len()
    }
}

#[cfg(not(any(feature = "error-correction", feature = "symspell")))]
impl HybridCorrector {
    pub fn new(whitelist: Vec<String>) -> Self {
        Self {
            symspell_whitelist: Arc::new(whitelist),
            last_backend: None,
        }
    }
}

#[cfg(all(feature = "error-correction", feature = "symspell"))]
impl Corrector for HybridCorrector {
    /// Without language context, prefer T5 if available, otherwise passthrough.
    fn correct(&mut self, prompt: &str) -> crate::error::Result<String> {
        if let Some(ref mut t5) = self.t5 {
            self.last_backend = Some("T5".to_string());
            return t5.correct(prompt);
        }
        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn correct_with_language(
        &mut self,
        prompt: &str,
        language: &str,
    ) -> crate::error::Result<String> {
        if language == "en" {
            if let Some(ref mut t5) = self.t5 {
                self.last_backend = Some("T5".to_string());
                return t5.correct(prompt);
            }
            self.last_backend = None;
            return Ok(prompt.to_string());
        }

        let whitelisted = self.symspell_whitelist.iter().any(|l| l == language);
        if whitelisted {
            if let Some(corrector) = self.symspell.get_mut(language) {
                self.last_backend = Some(corrector.name().to_string());
                return corrector.correct(prompt);
            }
        }

        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn name(&self) -> &str {
        self.last_backend.as_deref().unwrap_or("hybrid")
    }
}

#[cfg(feature = "error-correction")]
#[cfg(not(feature = "symspell"))]
impl Corrector for HybridCorrector {
    /// Without language context, prefer T5 if available, otherwise passthrough.
    fn correct(&mut self, prompt: &str) -> crate::error::Result<String> {
        if let Some(ref mut t5) = self.t5 {
            self.last_backend = Some("T5".to_string());
            t5.correct(prompt)
        } else {
            self.last_backend = None;
            Ok(prompt.to_string())
        }
    }

    fn correct_with_language(
        &mut self,
        prompt: &str,
        language: &str,
    ) -> crate::error::Result<String> {
        if language == "en" {
            if let Some(ref mut t5) = self.t5 {
                self.last_backend = Some("T5".to_string());
                return t5.correct(prompt);
            }
        }
        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn name(&self) -> &str {
        self.last_backend.as_deref().unwrap_or("hybrid")
    }
}

#[cfg(feature = "symspell")]
#[cfg(not(feature = "error-correction"))]
impl Corrector for HybridCorrector {
    /// Without language context, passthrough (no way to pick the right dictionary).
    fn correct(&mut self, prompt: &str) -> crate::error::Result<String> {
        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn correct_with_language(
        &mut self,
        prompt: &str,
        language: &str,
    ) -> crate::error::Result<String> {
        if self.symspell_whitelist.iter().any(|l| l == language)
            && let Some(corrector) = self.symspell.get_mut(language)
        {
            self.last_backend = Some(corrector.name().to_string());
            return corrector.correct(prompt);
        }

        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn name(&self) -> &str {
        self.last_backend.as_deref().unwrap_or("hybrid")
    }
}

#[cfg(not(any(feature = "error-correction", feature = "symspell")))]
impl Corrector for HybridCorrector {
    fn correct(&mut self, prompt: &str) -> crate::error::Result<String> {
        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn correct_with_language(
        &mut self,
        prompt: &str,
        _language: &str,
    ) -> crate::error::Result<String> {
        self.last_backend = None;
        Ok(prompt.to_string())
    }

    fn name(&self) -> &str {
        self.last_backend.as_deref().unwrap_or("hybrid")
    }
}

#[cfg(all(test, feature = "symspell"))]
mod tests {
    use super::*;

    struct MockCorrector {
        prefix: String,
    }

    impl MockCorrector {
        fn new(prefix: impl Into<String>) -> Self {
            Self {
                prefix: prefix.into(),
            }
        }
    }

    impl Corrector for MockCorrector {
        fn correct(&mut self, prompt: &str) -> crate::error::Result<String> {
            Ok(format!("{}:{}", self.prefix, prompt))
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    #[cfg(all(feature = "error-correction", feature = "symspell"))]
    #[test]
    fn hybrid_english_uses_t5() {
        let mut corrector = HybridCorrector::new(
            Some(Box::new(MockCorrector::new("T5")) as Box<dyn Corrector>),
            std::collections::HashMap::new(),
            Vec::new(),
        );
        let result = corrector.correct_with_language("test", "en").unwrap();
        assert_eq!(result, "T5:test");
    }

    #[cfg(all(feature = "error-correction", feature = "symspell"))]
    #[test]
    fn hybrid_whitelisted_uses_symspell() {
        let mut symspell = std::collections::HashMap::new();
        symspell.insert(
            "he".to_string(),
            Box::new(MockCorrector::new("SS-he")) as Box<dyn Corrector>,
        );
        let whitelist = vec!["he".to_string(), "ar".to_string()];
        let mut corrector = HybridCorrector::new(None, symspell, whitelist);
        let result = corrector.correct_with_language("Shalom", "he").unwrap();
        assert_eq!(result, "SS-he:Shalom");
    }

    #[cfg(all(feature = "error-correction", feature = "symspell"))]
    #[test]
    fn hybrid_non_whitelisted_uses_t5_fallback() {
        let mut symspell = std::collections::HashMap::new();
        symspell.insert(
            "de".to_string(),
            Box::new(MockCorrector::new("SS-de")) as Box<dyn Corrector>,
        );
        let whitelist = vec!["he".to_string(), "ar".to_string()];
        let mut corrector = HybridCorrector::new(
            Some(Box::new(MockCorrector::new("T5")) as Box<dyn Corrector>),
            symspell,
            whitelist,
        );
        let result = corrector.correct_with_language("Hallo", "de").unwrap();
        assert_eq!(result, "T5:Hallo");
    }

    #[cfg(all(feature = "error-correction", feature = "symspell"))]
    #[test]
    fn hybrid_supports_multiple_languages() {
        let mut symspell = std::collections::HashMap::new();
        symspell.insert(
            "he".to_string(),
            Box::new(MockCorrector::new("SS-he")) as Box<dyn Corrector>,
        );
        symspell.insert(
            "ar".to_string(),
            Box::new(MockCorrector::new("SS-ar")) as Box<dyn Corrector>,
        );
        let whitelist = vec!["he".to_string(), "ar".to_string()];
        let mut corrector = HybridCorrector::new(None, symspell, whitelist);

        assert_eq!(
            corrector.correct_with_language("Shalom", "he").unwrap(),
            "SS-he:Shalom"
        );
        assert_eq!(
            corrector.correct_with_language("Marhaba", "ar").unwrap(),
            "SS-ar:Marhaba"
        );
    }

    #[cfg(feature = "error-correction")]
    #[test]
    fn hybrid_corrector_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<HybridCorrector>();
    }
}
