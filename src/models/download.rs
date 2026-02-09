//! Model download and installation management.
//!
//! Handles downloading Whisper models from HuggingFace, verifying their integrity,
//! and storing them in the user's cache directory.

use crate::error::{Result, VoicshError};
use crate::models::catalog::{ModelInfo, get_model};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Get the directory where models are stored.
///
/// Uses `~/.cache/voicsh/models/` on Linux/Unix.
pub fn models_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("voicsh")
        .join("models")
}

/// Get the full path for a model file.
///
/// Always returns a path regardless of whether the model is in the catalog.
/// The file may or may not exist on disk.
pub fn model_path(name: &str) -> PathBuf {
    let resolved = crate::models::catalog::resolve_name(name);
    let filename = format!("ggml-{resolved}.bin");
    models_dir().join(filename)
}

/// Check if a model is installed.
pub fn is_model_installed(name: &str) -> bool {
    model_path(name).exists()
}

/// Core download: fetch url, save to path, verify sha1 if non-empty.
async fn download_to_path(
    name: &str,
    url: &str,
    sha1: &str,
    size_mb: u32,
    output_path: &Path,
    progress: bool,
) -> Result<()> {
    // Create models directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| VoicshError::Other(format!("Failed to create models directory: {e}")))?;
    }

    if progress {
        eprintln!("Downloading {name} ({size_mb} MB)...");
    }

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| VoicshError::Other(format!("Failed to start download: {e}")))?;

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
            // SAFETY: hardcoded template string — always valid
            #[allow(clippy::expect_used)]
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .expect("hardcoded progress bar template")
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Download with streaming and hash calculation
    let mut hasher = Sha1::new();
    let mut stream = response.bytes_stream();
    let mut file = fs::File::create(output_path)
        .map_err(|e| VoicshError::Other(format!("Failed to create output file: {e}")))?;

    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| VoicshError::Other(format!("Failed to read download chunk: {e}")))?;

        file.write_all(&chunk)
            .map_err(|e| VoicshError::Other(format!("Failed to write to file: {e}")))?;

        hasher.update(&chunk);

        if let Some(ref pb) = pb {
            pb.inc(chunk.len() as u64);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Downloaded");
    }

    // Verify SHA-1 checksum
    if !sha1.is_empty() {
        let calculated_hash = format!("{:x}", hasher.finalize());
        if calculated_hash != sha1 {
            if let Err(e) = fs::remove_file(output_path) {
                eprintln!("voicsh: failed to remove corrupted download: {e}");
            }
            return Err(VoicshError::Other(format!(
                "SHA-1 checksum mismatch. Expected: {sha1}, got: {calculated_hash}"
            )));
        }
        if progress {
            eprintln!("Checksum verified");
        }
    }

    if progress {
        eprintln!("Model installed to: {}", output_path.display());
    }

    Ok(())
}

/// Download a Whisper model.
///
/// Tries the static catalog first, then falls back to HuggingFace remote
/// discovery for models not in the catalog.
///
/// # Errors
///
/// Returns an error if:
/// - The model is not found in catalog or on HuggingFace
/// - The download fails
/// - The SHA-1 checksum doesn't match (if provided in catalog)
/// - The file cannot be written
pub async fn download_model(name: &str, progress: bool) -> Result<PathBuf> {
    let path = model_path(name);

    if path.exists() {
        if !progress {
            eprintln!(
                "Model '{}' is already installed at {}",
                name,
                path.display()
            );
        }
        return Ok(path);
    }

    // Try static catalog first
    if let Some(info) = get_model(name) {
        download_to_path(name, &info.url(), info.sha1, info.size_mb, &path, progress).await?;
        return Ok(path);
    }

    // Fall back to remote discovery
    remote_fallback(name, &path, progress).await
}

#[cfg(feature = "model-download")]
async fn remote_fallback(name: &str, path: &Path, progress: bool) -> Result<PathBuf> {
    let remote = crate::models::remote::fetch_remote_models()
        .await
        .map_err(|e| {
            VoicshError::Other(format!(
                "Model '{name}' not in catalog and remote fetch failed: {e}"
            ))
        })?;

    let rm = remote.iter().find(|m| m.name == name).ok_or_else(|| {
        VoicshError::Other(format!(
            "Model '{name}' not found in catalog or on HuggingFace.\n\
             Run 'voicsh models list' to see available models."
        ))
    })?;

    download_to_path(name, &rm.url, "", rm.size_mb, path, progress).await?;
    Ok(path.to_path_buf())
}

#[cfg(not(feature = "model-download"))]
async fn remote_fallback(name: &str, _path: &Path, _progress: bool) -> Result<PathBuf> {
    Err(VoicshError::Other(format!(
        "Model '{name}' not found in catalog.\n\
         Run 'voicsh models list' to see available models."
    )))
}

/// Find any installed model from the catalog.
///
/// Scans through all catalog models and returns the first one that is installed.
/// Useful for fallback scenarios when the configured model is not available.
pub fn find_any_installed_model() -> Option<String> {
    crate::models::catalog::list_models()
        .iter()
        .find(|m| is_model_installed(m.name))
        .map(|m| m.name.to_string())
}

/// List all installed model names by scanning the models directory.
///
/// Discovers every `ggml-*.bin` file, not just catalog models.
/// Returns model names (with the `ggml-` prefix and `.bin` suffix stripped).
pub fn list_installed_models() -> Vec<String> {
    let dir = models_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut names: Vec<String> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            let model = name.strip_prefix("ggml-")?.strip_suffix(".bin")?;
            if entry.path().is_file() {
                Some(model.to_string())
            } else {
                None
            }
        })
        .collect();

    names.sort();
    names
}

/// Format model information for display.
pub fn format_model_info(model: &ModelInfo) -> String {
    let status = if is_model_installed(model.name) {
        "[installed]"
    } else {
        "[not installed]"
    };
    format!("{:12} {:5} MB   {}", model.name, model.size_mb, status)
}

/// Format a remote model for display.
#[cfg(feature = "model-download")]
pub fn format_remote_model(name: &str, size_mb: u32) -> String {
    let status = if is_model_installed(name) {
        "[installed]"
    } else {
        "[not installed]"
    };
    format!("{:12} {:5} MB   {}", name, size_mb, status)
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
        assert!(path.to_string_lossy().contains("ggml-tiny.en.bin"));
    }

    #[test]
    fn test_model_path_for_unknown_model() {
        let path = model_path("nonexistent");
        assert!(path.to_string_lossy().contains("ggml-nonexistent.bin"));
    }

    #[test]
    fn test_model_path_resolves_alias() {
        let path = model_path("large");
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("large-v3-turbo"),
            "model_path(\"large\") should resolve to large-v3-turbo, got: {}",
            path_str
        );
    }

    #[test]
    fn test_is_model_installed_returns_false_for_invalid_model() {
        let installed = is_model_installed("nonexistent_model_xyz");
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
        assert!(formatted.contains("installed"));
    }

    #[test]
    fn test_model_path_filename_format() {
        let models = crate::models::catalog::list_models();
        for model in models {
            let path = model_path(model.name);
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
    fn test_find_any_installed_model_returns_option() {
        // This test verifies the function runs without panic.
        // It may return Some or None depending on local installation.
        let result = find_any_installed_model();
        if let Some(name) = result {
            assert!(
                crate::models::catalog::get_model(&name).is_some(),
                "Returned model {} should be in catalog",
                name
            );
        }
    }

    #[cfg(feature = "model-download")]
    #[test]
    fn test_format_remote_model_shows_name_and_size() {
        let formatted = format_remote_model("large-v3-q5_0", 1080);
        assert!(formatted.contains("large-v3-q5_0"));
        assert!(formatted.contains("1080"));
        assert!(formatted.contains("MB"));
        assert!(formatted.contains("installed"));
    }

    #[test]
    fn test_list_installed_models_returns_sorted_names() {
        let installed = list_installed_models();
        // Verify sorted order
        let mut sorted = installed.clone();
        sorted.sort();
        assert_eq!(
            installed, sorted,
            "list_installed_models should return sorted names"
        );
        // Every returned name should correspond to an existing file.
        // Note: we check the literal filename here, not model_path() which may resolve aliases.
        let dir = models_dir();
        for name in &installed {
            let literal_path = dir.join(format!("ggml-{}.bin", name));
            assert!(
                literal_path.exists(),
                "Listed model '{}' should exist on disk at {}",
                name,
                literal_path.display()
            );
        }
    }

    #[test]
    fn test_list_installed_models_strips_prefix_and_suffix() {
        // If any models are installed, their names should not contain ggml- or .bin
        for name in list_installed_models() {
            assert!(
                !name.starts_with("ggml-"),
                "Model name '{}' should not have ggml- prefix",
                name
            );
            assert!(
                !name.ends_with(".bin"),
                "Model name '{}' should not have .bin suffix",
                name
            );
        }
    }
}
