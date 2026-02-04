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
    /// SHA-256 checksum for integrity verification
    pub sha256: &'static str,
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
        sha256: "sha256_tiny_en_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "tiny",
        size_mb: 75,
        sha256: "sha256_tiny_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        english_only: false,
    },
    ModelInfo {
        name: "base.en",
        size_mb: 142,
        sha256: "sha256_base_en_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "base",
        size_mb: 142,
        sha256: "sha256_base_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        english_only: false,
    },
    ModelInfo {
        name: "small.en",
        size_mb: 466,
        sha256: "sha256_small_en_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "small",
        size_mb: 466,
        sha256: "sha256_small_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        english_only: false,
    },
    ModelInfo {
        name: "medium.en",
        size_mb: 1533,
        sha256: "sha256_medium_en_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        english_only: true,
    },
    ModelInfo {
        name: "medium",
        size_mb: 1533,
        sha256: "sha256_medium_placeholder",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        english_only: false,
    },
    ModelInfo {
        name: "large",
        size_mb: 3094,
        sha256: "sha256_large_placeholder",
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
/// The default is `base.en` - a good balance between speed and accuracy
/// for English-language use cases.
///
/// # Returns
///
/// The default model info.
pub fn default_model() -> &'static ModelInfo {
    get_model("base.en").expect("base.en model should always be present in catalog")
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
    fn test_default_model_is_base_en() {
        let default = default_model();
        assert_eq!(default.name, "base.en");
        assert_eq!(default.size_mb, 142);
        assert!(default.english_only);
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
        assert!(get_model("tiny.en").is_some());
        assert!(get_model("Tiny.en").is_none());
        assert!(get_model("TINY.EN").is_none());
    }
}
