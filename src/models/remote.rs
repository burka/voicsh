//! Dynamic model discovery from HuggingFace.
//!
//! Fetches the list of available ggml Whisper models from the
//! `ggerganov/whisper.cpp` repository on HuggingFace.

use crate::error::{Result, VoicshError};

const HF_TREE_URL: &str = "https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main";

/// A model discovered on HuggingFace.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteModel {
    pub name: String,
    pub size_mb: u32,
    pub url: String,
    pub english_only: bool,
}

/// Fetch available ggml models from HuggingFace.
///
/// Queries the HuggingFace API for files in the `ggerganov/whisper.cpp` repo,
/// filters for `ggml-*.bin` (excluding `.mlmodelc.zip`), and returns metadata
/// for each model.
///
/// # Errors
///
/// Returns an error on network failure or unexpected response format.
pub async fn fetch_remote_models() -> Result<Vec<RemoteModel>> {
    let client = reqwest::Client::new();
    let response =
        client.get(HF_TREE_URL).send().await.map_err(|e| {
            VoicshError::Other(format!("Failed to fetch HuggingFace model list: {e}"))
        })?;

    if !response.status().is_success() {
        return Err(VoicshError::Other(format!(
            "HuggingFace API returned status {}",
            response.status()
        )));
    }

    let text = response
        .text()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to read HuggingFace response: {e}")))?;

    let entries: Vec<serde_json::Value> = serde_json::from_str(&text)
        .map_err(|e| VoicshError::Other(format!("Failed to parse HuggingFace response: {e}")))?;

    let mut models = Vec::new();
    for entry in &entries {
        let path = match entry.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };

        if !path.starts_with("ggml-") || !path.ends_with(".bin") {
            continue;
        }

        let name = path
            .strip_prefix("ggml-")
            .and_then(|s| s.strip_suffix(".bin"))
            .unwrap_or(path);

        // Get size from LFS metadata or top-level size field
        let size_bytes = entry
            .get("lfs")
            .and_then(|lfs| lfs.get("size"))
            .and_then(|v| v.as_u64())
            .or_else(|| entry.get("size").and_then(|v| v.as_u64()))
            .unwrap_or(0);

        let size_mb = (size_bytes / (1024 * 1024)) as u32;

        // English-only models have ".en" before any version suffix
        let english_only = name.contains(".en");

        let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{path}");

        models.push(RemoteModel {
            name: name.to_string(),
            size_mb,
            url,
            english_only,
        });
    }

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_name_from_path() {
        let path = "ggml-large-v3-turbo.bin";
        let name = path
            .strip_prefix("ggml-")
            .and_then(|s| s.strip_suffix(".bin"))
            .unwrap();
        assert_eq!(name, "large-v3-turbo");
    }

    #[test]
    fn test_english_only_detection() {
        assert!("tiny.en".contains(".en"));
        assert!("base.en".contains(".en"));
        assert!(!"large-v3".contains(".en"));
        assert!(!"large-v3-turbo".contains(".en"));
    }

    #[test]
    fn test_remote_model_struct() {
        let model = RemoteModel {
            name: "large-v3-turbo".to_string(),
            size_mb: 1620,
            url:
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
                    .to_string(),
            english_only: false,
        };
        assert_eq!(model.name, "large-v3-turbo");
        assert_eq!(model.size_mb, 1620);
        assert!(!model.english_only);
        assert!(model.url.contains("ggml-large-v3-turbo.bin"));
    }

    #[test]
    fn test_size_conversion() {
        // 1620 MB in bytes
        let size_bytes: u64 = 1_699_020_800;
        let size_mb = (size_bytes / (1024 * 1024)) as u32;
        assert_eq!(size_mb, 1620);
    }

    #[test]
    fn test_hf_tree_url_is_valid() {
        assert!(HF_TREE_URL.starts_with("https://"));
        assert!(HF_TREE_URL.contains("huggingface.co"));
        assert!(HF_TREE_URL.contains("ggerganov/whisper.cpp"));
    }
}
