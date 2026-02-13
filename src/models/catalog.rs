//! Whisper model metadata catalog.
//!
//! This module provides a catalog of available Whisper models from OpenAI,
//! including model information, availability checks, and defaults.

/// Base URL for downloading Whisper GGML models from HuggingFace.
pub const HF_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Metadata for a Whisper model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// Model identifier (e.g., "tiny.en", "base", "large-v3-turbo")
    pub name: &'static str,
    /// Model size in megabytes
    pub size_mb: u32,
    /// SHA-1 checksum for integrity verification
    pub sha1: &'static str,
    /// Whether this model supports English only
    pub english_only: bool,
    /// Whether this is a quantized model
    pub quantized: bool,
}

impl ModelInfo {
    /// Filename for the model binary: `ggml-{name}.bin`.
    pub fn filename(&self) -> String {
        format!("ggml-{}.bin", self.name)
    }

    /// Full download URL derived from [`HF_BASE_URL`] and [`Self::filename`].
    pub fn url(&self) -> String {
        format!("{HF_BASE_URL}/{}", self.filename())
    }
}

/// Catalog of available Whisper models.
///
/// Models range from tiny (75 MB, fast, lower accuracy) to large (3094 MB, slower, highest accuracy).
/// The `.en` suffix indicates English-only models, which are faster and smaller.
pub const MODELS: &[ModelInfo] = &[
    // Standard models
    ModelInfo {
        name: "tiny.en",
        size_mb: 75,
        sha1: "c78c86eb1a8faa21b369bcd33207cc90d64ae9df",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        name: "tiny",
        size_mb: 75,
        sha1: "bd577a113a864445d4c299885e0cb97d4ba92b5f",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "base.en",
        size_mb: 142,
        sha1: "137c40403d78fd54d454da0f9bd998f78703390c",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        name: "base",
        size_mb: 142,
        sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "small.en",
        size_mb: 466,
        sha1: "db8a495a91d927739e50b3fc1cc4c6b8f6c2d022",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        name: "small",
        size_mb: 466,
        sha1: "55356645c2b361a969dfd0ef2c5a50d530afd8d5",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "medium.en",
        size_mb: 1533,
        sha1: "8c30f0e44ce9560643ebd10bbe50cd20eafd3723",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        name: "medium",
        size_mb: 1533,
        sha1: "fd9727b6e1217c2f614f9b698455c4ffd82463b4",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "large-v3-turbo",
        size_mb: 1620,
        sha1: "",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "large-v1",
        size_mb: 3094,
        sha1: "",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "large-v2",
        size_mb: 3094,
        sha1: "",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        name: "large-v3",
        size_mb: 3095,
        sha1: "",
        english_only: false,
        quantized: false,
    },
    // Quantized models - Q5_1
    ModelInfo {
        name: "tiny-q5_1",
        size_mb: 32,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "tiny.en-q5_1",
        size_mb: 32,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "base-q5_1",
        size_mb: 59,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "base.en-q5_1",
        size_mb: 59,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "small-q5_1",
        size_mb: 190,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "small.en-q5_1",
        size_mb: 190,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    // Quantized models - Q5_0
    ModelInfo {
        name: "medium-q5_0",
        size_mb: 539,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "medium.en-q5_0",
        size_mb: 539,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "large-v2-q5_0",
        size_mb: 1080,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "large-v3-q5_0",
        size_mb: 1080,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    // Quantized models - Q8_0
    ModelInfo {
        name: "tiny-q8_0",
        size_mb: 43,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "tiny.en-q8_0",
        size_mb: 43,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "base-q8_0",
        size_mb: 81,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "base.en-q8_0",
        size_mb: 81,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "small-q8_0",
        size_mb: 264,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "small.en-q8_0",
        size_mb: 264,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "medium-q8_0",
        size_mb: 823,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "medium.en-q8_0",
        size_mb: 823,
        sha1: "",
        english_only: true,
        quantized: true,
    },
    ModelInfo {
        name: "large-v2-q8_0",
        size_mb: 1660,
        sha1: "",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        name: "large-v3-turbo-q8_0",
        size_mb: 874,
        sha1: "",
        english_only: false,
        quantized: true,
    },
];

/// Resolve legacy model name aliases.
///
/// Maps `"large"` → `"large-v3-turbo"` for backwards compatibility with
/// existing `config.toml` files. All other names pass through unchanged.
pub fn resolve_name(name: &str) -> &str {
    match name {
        "large" => "large-v3-turbo",
        other => other,
    }
}

/// Find a model by name.
///
/// Resolves aliases (e.g., `"large"` → `"large-v3-turbo"`) before lookup.
///
/// # Arguments
///
/// * `name` - Model identifier (e.g., "tiny.en", "base", "large")
///
/// # Returns
///
/// Returns `Some(&ModelInfo)` if the model exists, `None` otherwise.
pub fn get_model(name: &str) -> Option<&'static ModelInfo> {
    let resolved = resolve_name(name);
    MODELS.iter().find(|m| m.name == resolved)
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
/// - `"large"` → `Some("large-v3-turbo")` (alias resolved)
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
        assert_eq!(models.len(), 32);
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
        assert_eq!(multilingual_variant("large"), Some("large-v3-turbo"));
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
            let url = model.url();
            assert!(
                url.starts_with("https://"),
                "Model {} has invalid URL: {}",
                model.name,
                url
            );
            assert!(
                url.contains("huggingface.co"),
                "Model {} URL not from HuggingFace: {}",
                model.name,
                url
            );
        }
    }

    #[test]
    fn test_english_models_have_en_suffix() {
        for model in list_models() {
            if model.english_only {
                assert!(
                    model.name.contains(".en"),
                    "English-only model {} should contain .en in name",
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
            ("large", 1620),
            ("large-v1", 3094),
            ("large-v2", 3094),
            ("large-v3", 3095),
            // Quantized models
            ("tiny-q5_1", 32),
            ("tiny.en-q5_1", 32),
            ("base-q5_1", 59),
            ("base.en-q5_1", 59),
            ("small-q5_1", 190),
            ("small.en-q5_1", 190),
            ("medium-q5_0", 539),
            ("medium.en-q5_0", 539),
            ("large-v2-q5_0", 1080),
            ("large-v3-q5_0", 1080),
            ("tiny-q8_0", 43),
            ("tiny.en-q8_0", 43),
            ("base-q8_0", 81),
            ("base.en-q8_0", 81),
            ("small-q8_0", 264),
            ("small.en-q8_0", 264),
            ("medium-q8_0", 823),
            ("medium.en-q8_0", 823),
            ("large-v2-q8_0", 1660),
            ("large-v3-turbo-q8_0", 874),
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

    #[test]
    fn test_resolve_name_alias() {
        assert_eq!(resolve_name("large"), "large-v3-turbo");
    }

    #[test]
    fn test_resolve_name_passthrough() {
        assert_eq!(resolve_name("tiny"), "tiny");
        assert_eq!(resolve_name("base.en"), "base.en");
        assert_eq!(resolve_name("large-v3-turbo"), "large-v3-turbo");
        assert_eq!(resolve_name("unknown"), "unknown");
    }

    #[test]
    fn test_url_derived_from_name() {
        for model in list_models() {
            let expected = format!("{HF_BASE_URL}/ggml-{}.bin", model.name);
            assert_eq!(
                model.url(),
                expected,
                "Model {} URL should be derived from name",
                model.name
            );
        }
    }

    #[test]
    fn test_quantized_models_are_flagged() {
        for model in list_models() {
            if model.name.contains("q5") || model.name.contains("q8") {
                assert!(
                    model.quantized,
                    "Model {} contains q5/q8 but quantized=false",
                    model.name
                );
            }
        }
    }

    #[test]
    fn test_standard_models_not_quantized() {
        for model in list_models() {
            if !model.name.contains("q5") && !model.name.contains("q8") {
                assert!(
                    !model.quantized,
                    "Model {} does not contain q but quantized=true",
                    model.name
                );
            }
        }
    }
}
