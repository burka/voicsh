//! Catalog of available T5 error correction models.

/// Metadata for a T5 error correction model.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionModelInfo {
    /// Short name used in config and CLI (e.g. "flan-t5-small").
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Approximate download size in MB.
    pub size_mb: u32,
    /// HuggingFace repository containing the model.
    pub hf_repo: &'static str,
    /// GGUF model filename within the repository.
    pub hf_filename: &'static str,
    /// JSON config filename within the repository.
    pub config_filename: &'static str,
    /// Short description with expected latency.
    pub description: &'static str,
}

/// Shared tokenizer filename — all Flan-T5 variants use the same tokenizer.
pub const TOKENIZER_FILENAME: &str = "tokenizer.json";

/// HuggingFace repository for quantized T5 models.
pub const CORRECTION_MODEL_REPO: &str = "lmz/candle-quantized-t5";

/// Available correction models, ordered by size (smallest first).
pub const CORRECTION_MODELS: &[CorrectionModelInfo] = &[
    CorrectionModelInfo {
        name: "flan-t5-small",
        display_name: "Flan-T5 Small (English, 64 MB)",
        size_mb: 64,
        hf_repo: CORRECTION_MODEL_REPO,
        hf_filename: "model.gguf",
        config_filename: "config.json",
        description: "Fast, lower quality. ~50-150 ms per correction on CPU.",
    },
    CorrectionModelInfo {
        name: "flan-t5-base",
        display_name: "Flan-T5 Base (English, 263 MB)",
        size_mb: 263,
        hf_repo: CORRECTION_MODEL_REPO,
        hf_filename: "model-flan-t5-base.gguf",
        config_filename: "config-flan-t5-base.json",
        description: "Balanced speed and quality. ~150-400 ms per correction on CPU.",
    },
    CorrectionModelInfo {
        name: "flan-t5-large",
        display_name: "Flan-T5 Large (English, 852 MB)",
        size_mb: 852,
        hf_repo: CORRECTION_MODEL_REPO,
        hf_filename: "model-flan-t5-large.gguf",
        config_filename: "config-flan-t5-large.json",
        description: "Best quality, slower. ~400-1000 ms per correction on CPU.",
    },
];

/// Look up a correction model by name.
pub fn get_correction_model(name: &str) -> Option<&'static CorrectionModelInfo> {
    CORRECTION_MODELS.iter().find(|m| m.name == name)
}

/// List all available correction models.
pub fn list_correction_models() -> &'static [CorrectionModelInfo] {
    CORRECTION_MODELS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_correction_model_small() {
        let model = get_correction_model("flan-t5-small").expect("flan-t5-small should exist");
        assert_eq!(model.name, "flan-t5-small");
        assert_eq!(model.size_mb, 64);
        assert_eq!(model.hf_repo, CORRECTION_MODEL_REPO);
        assert_eq!(model.hf_filename, "model.gguf");
    }

    #[test]
    fn test_get_correction_model_base() {
        let model = get_correction_model("flan-t5-base").expect("flan-t5-base should exist");
        assert_eq!(model.name, "flan-t5-base");
        assert_eq!(model.size_mb, 263);
        assert_eq!(model.hf_filename, "model-flan-t5-base.gguf");
    }

    #[test]
    fn test_get_correction_model_large() {
        let model = get_correction_model("flan-t5-large").expect("flan-t5-large should exist");
        assert_eq!(model.name, "flan-t5-large");
        assert_eq!(model.size_mb, 852);
    }

    #[test]
    fn test_get_correction_model_nonexistent() {
        assert!(get_correction_model("nonexistent").is_none());
    }

    #[test]
    fn test_list_correction_models_count() {
        let models = list_correction_models();
        assert_eq!(models.len(), 3);
    }

    #[test]
    fn test_list_correction_models_ordered_by_size() {
        let models = list_correction_models();
        for window in models.windows(2) {
            assert!(
                window[0].size_mb < window[1].size_mb,
                "{} ({} MB) should come before {} ({} MB)",
                window[0].name,
                window[0].size_mb,
                window[1].name,
                window[1].size_mb,
            );
        }
    }

    #[test]
    fn test_all_models_share_tokenizer_repo() {
        for model in CORRECTION_MODELS {
            assert_eq!(
                model.hf_repo, CORRECTION_MODEL_REPO,
                "{} should use shared repo",
                model.name
            );
        }
    }

    #[test]
    fn test_correction_model_info_clone() {
        let model = get_correction_model("flan-t5-small").expect("should exist");
        let cloned = model.clone();
        assert_eq!(model, &cloned);
    }
}
