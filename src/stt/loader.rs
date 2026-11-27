//! Open the speech-to-text backend a catalog model needs.
//!
//! Whisper models (and model files outside the catalog) load with whisper.cpp;
//! Parakeet TDT and Nemotron snapshot models load with ONNX Runtime when the
//! `parakeet` feature is enabled.

use crate::error::{Result, VoicshError};
use crate::models::catalog::{HfSnapshot, ModelBackend, get_model};
use crate::stt::transcriber::Transcriber;
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Backend-independent transcriber settings.
#[derive(Debug, Clone)]
pub struct TranscriberSettings {
    /// Language code (e.g. "de", "en") or "auto".
    pub language: String,
    /// Inference threads (None = backend default).
    pub threads: Option<usize>,
    /// How long the model can sit idle in memory before unloading. `None` = never unload.
    pub idle_unload_after: Option<Duration>,
}

/// Load `model_name` from `model_path` with the backend its catalog entry names.
///
/// Names outside the catalog (e.g. a path to a ggml file) load with whisper.
///
/// # Errors
/// Fails if the model is missing or incomplete, can't be loaded, or needs a
/// backend this binary was built without.
pub fn open_transcriber(
    model_name: &str,
    model_path: PathBuf,
    settings: &TranscriberSettings,
) -> Result<Arc<dyn Transcriber>> {
    let backend = get_model(model_name).map_or(ModelBackend::Whisper, |model| model.backend);
    match backend {
        ModelBackend::Whisper => Ok(Arc::new(WhisperTranscriber::new(WhisperConfig {
            model_path,
            language: settings.language.clone(),
            threads: settings.threads,
            use_gpu: true,
            idle_unload_after: settings.idle_unload_after,
        })?)),
        ModelBackend::ParakeetTdt(snapshot) => open_parakeet(
            Architecture::Tdt,
            model_name,
            snapshot,
            model_path,
            settings,
        ),
        ModelBackend::Nemotron(snapshot) => open_parakeet(
            Architecture::Nemotron,
            model_name,
            snapshot,
            model_path,
            settings,
        ),
    }
}

#[cfg(feature = "parakeet")]
use crate::stt::parakeet::ParakeetArchitecture as Architecture;

/// Stand-in so dispatch compiles without the `parakeet` feature.
#[cfg(not(feature = "parakeet"))]
enum Architecture {
    Tdt,
    Nemotron,
}

#[cfg(feature = "parakeet")]
fn open_parakeet(
    architecture: Architecture,
    model_name: &str,
    snapshot: &HfSnapshot,
    model_dir: PathBuf,
    settings: &TranscriberSettings,
) -> Result<Arc<dyn Transcriber>> {
    use crate::stt::parakeet::{ParakeetConfig, ParakeetTranscriber};

    if let Some(missing) = missing_snapshot_file(snapshot, &model_dir) {
        return Err(VoicshError::Transcription {
            message: format!(
                "Model '{model_name}' is incomplete: {} is missing.\n\
                 Reinstall it: rm -r {} && voicsh models install {model_name}",
                missing.display(),
                model_dir.display()
            ),
        });
    }
    Ok(Arc::new(ParakeetTranscriber::new(ParakeetConfig {
        architecture,
        model_dir,
        language: settings.language.clone(),
        threads: settings.threads,
        idle_unload_after: settings.idle_unload_after,
    })?))
}

#[cfg(not(feature = "parakeet"))]
fn open_parakeet(
    _architecture: Architecture,
    model_name: &str,
    _snapshot: &HfSnapshot,
    _model_dir: PathBuf,
    _settings: &TranscriberSettings,
) -> Result<Arc<dyn Transcriber>> {
    Err(VoicshError::Transcription {
        message: format!(
            "Model '{model_name}' needs the ONNX Runtime backend, but this voicsh was built without it.\n\
             Rebuild with: cargo install voicsh --features parakeet\n\
             Or choose a whisper model: voicsh models list"
        ),
    })
}

/// First file of `snapshot` absent from `model_dir`, if any.
#[cfg(feature = "parakeet")]
fn missing_snapshot_file(snapshot: &HfSnapshot, model_dir: &std::path::Path) -> Option<PathBuf> {
    snapshot
        .files
        .iter()
        .map(|file| model_dir.join(file.name))
        .find(|path| !path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAKEET_TDT: &str = "parakeet-tdt-0.6b-v3-int8";

    fn settings() -> TranscriberSettings {
        TranscriberSettings {
            language: "auto".to_string(),
            threads: None,
            idle_unload_after: None,
        }
    }

    fn error_message(result: Result<Arc<dyn Transcriber>>) -> String {
        match result {
            Ok(transcriber) => panic!("expected an error, loaded {}", transcriber.model_name()),
            Err(VoicshError::Transcription { message }) => message,
            Err(other) => panic!("expected VoicshError::Transcription, got {other:?}"),
        }
    }

    #[test]
    fn test_open_transcriber_loads_unknown_names_with_whisper() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ggml-custom.bin");

        let result = open_transcriber("custom", missing.clone(), &settings());

        // WhisperTranscriber reports the missing ggml file.
        match result {
            Err(VoicshError::TranscriptionModelNotFound { path }) => {
                assert_eq!(path, missing.display().to_string());
            }
            Err(other) => panic!("expected TranscriptionModelNotFound, got {other:?}"),
            Ok(t) => panic!("expected an error, loaded {}", t.model_name()),
        }
    }

    #[cfg(not(feature = "parakeet"))]
    #[test]
    fn test_open_transcriber_without_parakeet_feature_explains_rebuild() {
        let dir = tempfile::tempdir().unwrap();

        let message = error_message(open_transcriber(
            PARAKEET_TDT,
            dir.path().to_path_buf(),
            &settings(),
        ));

        assert!(
            message.starts_with(&format!(
                "Model '{PARAKEET_TDT}' needs the ONNX Runtime backend"
            )),
            "message: {message}"
        );
        assert!(
            message.contains("--features parakeet"),
            "message: {message}"
        );
    }

    #[cfg(feature = "parakeet")]
    #[test]
    fn test_open_transcriber_reports_first_missing_snapshot_file() {
        let dir = tempfile::tempdir().unwrap();
        let model_dir = dir.path().join(PARAKEET_TDT);
        std::fs::create_dir(&model_dir).unwrap();
        std::fs::write(model_dir.join("encoder-model.int8.onnx"), b"stub").unwrap();

        let message = error_message(open_transcriber(
            PARAKEET_TDT,
            model_dir.clone(),
            &settings(),
        ));

        let expected = format!(
            "Model '{PARAKEET_TDT}' is incomplete: {} is missing.\n\
             Reinstall it: rm -r {} && voicsh models install {PARAKEET_TDT}",
            model_dir.join("decoder_joint-model.int8.onnx").display(),
            model_dir.display()
        );
        assert_eq!(message, expected);
    }

    #[cfg(feature = "parakeet")]
    #[test]
    fn test_missing_snapshot_file_is_none_when_all_files_present() {
        let dir = tempfile::tempdir().unwrap();
        let ModelBackend::ParakeetTdt(snapshot) = get_model(PARAKEET_TDT).unwrap().backend else {
            panic!("{PARAKEET_TDT} must use the ParakeetTdt backend");
        };
        for file in snapshot.files {
            std::fs::write(dir.path().join(file.name), b"stub").unwrap();
        }

        assert_eq!(missing_snapshot_file(snapshot, dir.path()), None);
    }
}
