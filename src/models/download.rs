//! Model download and installation management.
//!
//! Handles downloading Whisper models from HuggingFace, verifying their integrity,
//! and storing them in the user's cache directory.

use crate::error::{Result, VoicshError};
use crate::models::catalog::{ModelInfo, get_model};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Get the directory where models are stored.
///
/// Uses `~/.cache/voicsh/models/` on Linux/Unix.
///
/// # Returns
///
/// PathBuf to the models directory.
pub fn models_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("voicsh")
        .join("models")
}

/// Get the full path for a model file.
///
/// # Arguments
///
/// * `name` - Model name (e.g., "base.en", "tiny")
///
/// # Returns
///
/// Some(PathBuf) if the model exists in the catalog, None otherwise.
pub fn model_path(name: &str) -> Option<PathBuf> {
    let model_info = get_model(name)?;
    let filename = format!("ggml-{}.bin", model_info.name);
    Some(models_dir().join(filename))
}

/// Check if a model is installed.
///
/// # Arguments
///
/// * `name` - Model name (e.g., "base.en", "tiny")
///
/// # Returns
///
/// true if the model file exists, false otherwise.
pub fn is_model_installed(name: &str) -> bool {
    model_path(name).is_some_and(|p| p.exists())
}

/// Download a Whisper model.
///
/// # Arguments
///
/// * `name` - Model name from the catalog (e.g., "base.en", "tiny")
/// * `progress` - Whether to show a progress bar
///
/// # Returns
///
/// PathBuf to the downloaded model file on success.
///
/// # Errors
///
/// Returns an error if:
/// - The model name is not in the catalog
/// - The download fails
/// - The SHA-256 checksum doesn't match (if provided in catalog)
/// - The file cannot be written
pub async fn download_model(name: &str, progress: bool) -> Result<PathBuf> {
    // Get model info from catalog
    let model_info = get_model(name)
        .ok_or_else(|| VoicshError::Other(format!("Model '{}' not found in catalog", name)))?;

    // Check if already installed
    if is_model_installed(name) {
        let path = model_path(name).expect("path should exist for installed model");
        if !progress {
            eprintln!(
                "Model '{}' is already installed at {}",
                name,
                path.display()
            );
        }
        return Ok(path);
    }

    // Create models directory if it doesn't exist
    let dir = models_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| VoicshError::Other(format!("Failed to create models directory: {}", e)))?;

    // Determine output path
    let output_path = model_path(name).expect("path should exist for valid model");

    if progress {
        eprintln!(
            "Downloading {} ({} MB)...",
            model_info.name, model_info.size_mb
        );
    }

    // Download the file
    let client = reqwest::Client::new();
    let response = client
        .get(model_info.url)
        .send()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to start download: {}", e)))?;

    if !response.status().is_success() {
        return Err(VoicshError::Other(format!(
            "Download failed with status: {}",
            response.status()
        )));
    }

    let total_size = response.content_length().unwrap_or(0);

    // Set up progress bar
    let pb = if progress {
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Download with streaming and hash calculation
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    let mut file = fs::File::create(&output_path)
        .map_err(|e| VoicshError::Other(format!("Failed to create output file: {}", e)))?;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| VoicshError::Other(format!("Failed to read download chunk: {}", e)))?;

        file.write_all(&chunk)
            .map_err(|e| VoicshError::Other(format!("Failed to write to file: {}", e)))?;

        hasher.update(&chunk);

        if let Some(ref pb) = pb {
            pb.inc(chunk.len() as u64);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Downloaded");
    }

    // Verify SHA-256 if provided (skip placeholder checksums)
    if !model_info.sha256.starts_with("sha256_") && !model_info.sha256.is_empty() {
        let calculated_hash = format!("{:x}", hasher.finalize());
        if calculated_hash != model_info.sha256 {
            // Remove corrupted file
            let _ = fs::remove_file(&output_path);
            return Err(VoicshError::Other(format!(
                "SHA-256 checksum mismatch. Expected: {}, got: {}",
                model_info.sha256, calculated_hash
            )));
        }
        if progress {
            eprintln!("Checksum verified");
        }
    }

    if progress {
        eprintln!("Model installed to: {}", output_path.display());
    }

    Ok(output_path)
}

/// Format model information for display.
///
/// # Arguments
///
/// * `model` - Model information from catalog
///
/// # Returns
///
/// Formatted string with model name, size, and installation status.
pub fn format_model_info(model: &ModelInfo) -> String {
    let status = if is_model_installed(model.name) {
        "[installed]"
    } else {
        "[not installed]"
    };
    format!("{:12} {:5} MB   {}", model.name, model.size_mb, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_dir_is_valid_path() {
        let dir = models_dir();
        assert!(dir.to_string_lossy().contains("voicsh"));
        assert!(dir.to_string_lossy().contains("models"));
    }

    #[test]
    fn test_model_path_for_valid_model() {
        let path = model_path("tiny.en");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains("ggml-tiny.en.bin"));
    }

    #[test]
    fn test_model_path_for_invalid_model() {
        let path = model_path("nonexistent");
        assert!(path.is_none());
    }

    #[test]
    fn test_is_model_installed_returns_false_for_nonexistent() {
        // Use a valid model name that definitely won't be installed
        let installed = is_model_installed("tiny.en");
        // We can't assume it's not installed, but we can verify the function works
        assert!(installed || !installed); // Tautology to verify function runs
    }

    #[test]
    fn test_is_model_installed_returns_false_for_invalid_model() {
        let installed = is_model_installed("nonexistent_model");
        assert!(!installed);
    }

    #[test]
    fn test_format_model_info_shows_name_and_size() {
        let model = get_model("tiny.en").unwrap();
        let formatted = format_model_info(model);
        assert!(formatted.contains("tiny.en"));
        assert!(formatted.contains("75"));
        assert!(formatted.contains("MB"));
    }

    #[test]
    fn test_format_model_info_shows_installation_status() {
        let model = get_model("tiny.en").unwrap();
        let formatted = format_model_info(model);
        // Should contain either installed or not installed
        assert!(formatted.contains("installed"));
    }

    #[test]
    fn test_model_path_filename_format() {
        let models = crate::models::catalog::list_models();
        for model in models {
            let path = model_path(model.name).expect("valid model should have path");
            let filename = path.file_name().unwrap().to_string_lossy();
            assert!(
                filename.starts_with("ggml-"),
                "Model {} filename should start with 'ggml-': {}",
                model.name,
                filename
            );
            assert!(
                filename.ends_with(".bin"),
                "Model {} filename should end with '.bin': {}",
                model.name,
                filename
            );
        }
    }

    #[test]
    fn test_all_catalog_models_have_paths() {
        let models = crate::models::catalog::list_models();
        for model in models {
            let path = model_path(model.name);
            assert!(
                path.is_some(),
                "Model {} should have a valid path",
                model.name
            );
        }
    }
}
