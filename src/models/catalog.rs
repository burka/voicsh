//! Whisper model metadata catalog.
//!
//! This module provides a catalog of available Whisper models from OpenAI,
//! including model information, availability checks, and defaults.

/// Metadata for a Whisper model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// Model identifier (e.g., "tiny.en", "base", "large")
    pub name: &'static str,
    /// Model size in megabytes
    pub size_mb: u32,
    /// SHA-1 checksum for integrity verification
    pub sha1: &'static str,
    /// Download URL from HuggingFace
    pub url: &'static str,
    /// Whether this model supports English only
    pub english_only: bool,
}

/// Catalog of available Whisper models.
///
/// Models range from tiny (75 MB, fast, lower accuracy) to large (3094 MB, slower, highest accuracy).
/// The `.en` suffix indicates English-only models, which are faster and smaller.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "tiny.en",
        size_mb: 75,
        sha1: "c78c86eb1a8faa21b369bcd33207cc90d64ae9df",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "tiny",
        size_mb: 75,
        sha1: "bd577a113a864445d4c299885e0cb97d4ba92b5f",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        english_only: false,
    },
    ModelInfo {
        name: "base.en",
        size_mb: 142,
        sha1: "137c40403d78fd54d454da0f9bd998f78703390c",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "base",
        size_mb: 142,
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        english_only: false,
    },
    ModelInfo {
        name: "small.en",
        size_mb: 466,
        sha1: "db8a495a91d927739e50b3fc1cc4c6b8f6c2d022",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "small",
        size_mb: 466,
        sha1: "55356645c2b361a969dfd0ef2c5a50d530afd8d5",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        english_only: false,
    },
    ModelInfo {
        name: "medium.en",
        size_mb: 1533,
        sha1: "8c30f0e44ce9560643ebd10bbe50cd20eafd3723",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "medium",
        size_mb: 1533,
        sha1: "fd9727b6e1217c2f614f9b698455c4ffd82463b4",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        english_only: false,
    },
    ModelInfo {
        name: "large",
        size_mb: 3094,
        sha1: "b1caaf735c4cc1429223d5a74f0f4d0b9b59a299",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large.bin",
        english_only: false,
    },
];

/// Find a model by name.
///
/// # Arguments
///
/// * `name` - Model identifier (e.g., "tiny.en", "base", "large")
///
/// # Returns
///
/// Returns `Some(&ModelInfo)` if the model exists, `None` otherwise.
pub fn get_model(name: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.name == name)
}

/// Get all available models.
///
/// # Returns
///
/// A slice of all available models in the catalog.
pub fn list_models() -> &'static [ModelInfo] {
    MODELS
}

/// Get the default recommended model.
///
/// The default is `base` (multilingual) — supports auto-detection of any language.
///
/// # Returns
///
/// The default model info.
pub fn default_model() -> &'static ModelInfo {
    // SAFETY: "base" is hardcoded in MODELS constant — always present.
    #[allow(clippy::expect_used)]
    get_model("base").expect("base model must be present in MODELS catalog")
}

/// Return the multilingual variant for a model name.
///
/// - `"base.en"` → `Some("base")`
/// - `"base"` → `Some("base")` (already multilingual)
/// - `"large"` → `Some("large")` (only multilingual exists)
/// - `"unknown"` → `None`
pub fn multilingual_variant(name: &str) -> Option<&'static str> {
    let base = name
        .strip_suffix(crate::defaults::ENGLISH_ONLY_SUFFIX)
        .unwrap_or(name);
    get_model(base).map(|m| m.name)
}

/// Return the English-only variant for a model name.
///
/// - `"base"` → `Some("base.en")`
/// - `"base.en"` → `Some("base.en")` (already English)
/// - `"large"` → `None` (no .en variant exists)
/// - `"unknown"` → `None`
pub fn english_variant(name: &str) -> Option<&'static str> {
    let base = name
        .strip_suffix(crate::defaults::ENGLISH_ONLY_SUFFIX)
        .unwrap_or(name);
    let en_name = format!("{}{}", base, crate::defaults::ENGLISH_ONLY_SUFFIX);
    get_model(&en_name).map(|m| m.name)
}

/// Resolve the model name based on the configured language.
///
/// Ensures a multilingual model is used when language is not English.
pub fn resolve_model_for_language(model: &str, language: &str, quiet: bool) -> String {
    let needs_multilingual = language == crate::defaults::AUTO_LANGUAGE
        || (language != crate::defaults::ENGLISH_LANGUAGE && !language.is_empty());
    let is_english_only = model.ends_with(crate::defaults::ENGLISH_ONLY_SUFFIX);

    if needs_multilingual
        && is_english_only
        && let Some(ml) = multilingual_variant(model)
    {
        if !quiet {
            eprintln!(
                "Switching model '{}' → '{}' (language='{}' needs multilingual model).",
                model, ml, language
            );
        }
        return ml.to_string();
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_exists() {
        let model = get_model("tiny.en");
        assert!(model.is_some());
        let model = model.unwrap();
        assert_eq!(model.name, "tiny.en");
        assert_eq!(model.size_mb, 75);
        assert!(model.english_only);
    }

    #[test]
    fn test_get_model_not_found() {
        let model = get_model("nonexistent");
        assert!(model.is_none());
    }

    #[test]
    fn test_list_models_not_empty() {
        let models = list_models();
        assert!(!models.is_empty());
        assert_eq!(models.len(), 9);
    }

    #[test]
    fn test_default_model_is_base() {
        let default = default_model();
        assert_eq!(default.name, "base");
        assert_eq!(default.size_mb, 142);
        assert!(!default.english_only);
    }

    #[test]
    fn test_multilingual_variant() {
        assert_eq!(multilingual_variant("base.en"), Some("base"));
        assert_eq!(multilingual_variant("base"), Some("base"));
        assert_eq!(multilingual_variant("small.en"), Some("small"));
        assert_eq!(multilingual_variant("large"), Some("large"));
        assert_eq!(multilingual_variant("unknown"), None);
    }

    #[test]
    fn test_english_variant() {
        assert_eq!(english_variant("base"), Some("base.en"));
        assert_eq!(english_variant("base.en"), Some("base.en"));
        assert_eq!(english_variant("small"), Some("small.en"));
        assert_eq!(english_variant("large"), None);
        assert_eq!(english_variant("unknown"), None);
    }

    #[test]
    fn test_all_models_have_valid_url() {
        for model in list_models() {
            assert!(
                model.url.starts_with("https://"),
                "Model {} has invalid URL: {}",
                model.name,
                model.url
            );
            assert!(
                model.url.contains("huggingface.co"),
                "Model {} URL not from HuggingFace: {}",
                model.name,
                model.url
            );
        }
    }

    #[test]
    fn test_english_models_have_en_suffix() {
        for model in list_models() {
            if model.english_only {
                assert!(
                    model.name.ends_with(".en"),
                    "English-only model {} should have .en suffix",
                    model.name
                );
            }
        }
    }

    #[test]
    fn test_model_sizes_are_correct() {
        let sizes = [
            ("tiny.en", 75),
            ("tiny", 75),
            ("base.en", 142),
            ("base", 142),
            ("small.en", 466),
            ("small", 466),
            ("medium.en", 1533),
            ("medium", 1533),
            ("large", 3094),
        ];

        for (name, expected_size) in sizes {
            let model = get_model(name).expect(&format!("Model {} not found", name));
            assert_eq!(
                model.size_mb, expected_size,
                "Model {} has wrong size",
                name
            );
        }
    }

    #[test]
    fn test_model_names_are_unique() {
        let names: Vec<_> = list_models().iter().map(|m| m.name).collect();
        let mut unique_names = names.clone();
        unique_names.sort_unstable();
        unique_names.dedup();
        assert_eq!(
            names.len(),
            unique_names.len(),
            "Model names are not unique"
        );
    }

    #[test]
    fn test_get_model_case_sensitive() {
        let model = get_model("tiny.en");
        assert!(model.is_some(), "tiny.en should exist");
        let model = model.unwrap();
        assert_eq!(model.name, "tiny.en", "Model name should match");
        assert_eq!(model.size_mb, 75, "Model size should be 75MB");

        assert!(
            get_model("Tiny.en").is_none(),
            "Uppercase T should not match"
        );
        assert!(get_model("TINY.EN").is_none(), "All caps should not match");
    }

    #[test]
    fn test_resolve_auto_with_english_model_switches_to_multilingual() {
        let result = resolve_model_for_language("base.en", "auto", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_non_english_with_english_model_switches() {
        let result = resolve_model_for_language("base.en", "de", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_english_with_english_model_keeps() {
        let result = resolve_model_for_language("base.en", "en", true);
        assert_eq!(result, "base.en");
    }

    #[test]
    fn test_resolve_auto_with_multilingual_model_keeps() {
        let result = resolve_model_for_language("base", "auto", true);
        assert_eq!(result, "base");
    }

    #[test]
    fn test_resolve_unknown_model_keeps_as_is() {
        let result = resolve_model_for_language("custom-model.en", "auto", true);
        // Unknown model, no catalog entry, keep as-is
        assert_eq!(result, "custom-model.en");
    }
}
