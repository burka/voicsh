//! NVIDIA Parakeet TDT and Nemotron speech-to-text via ONNX Runtime.
//!
//! Both models run on the CPU through [`parakeet_rs`]. Each pipeline chunk is
//! transcribed offline (independently); Nemotron's streaming mode is not used.
//!
//! # Feature Gate
//!
//! Requires the `parakeet` feature. The build downloads a prebuilt ONNX Runtime
//! (see DEVELOPMENT.md, "Parakeet / Nemotron backend").

use crate::audio::vad::{Clock, SystemClock};
use crate::defaults;
use crate::error::{Result, VoicshError};
use crate::stt::idle_tracker::IdleTracker;
use crate::stt::transcriber::{TokenProbability, Transcriber, TranscriptionResult, pcm_to_f32};
use parakeet_rs::ort::environment::Environment;
use parakeet_rs::{ExecutionConfig, Nemotron, ParakeetTDT, Transcriber as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Which model architecture a model directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParakeetArchitecture {
    /// Parakeet TDT: detects the language itself, no language prompt.
    Tdt,
    /// Nemotron ASR: prompted with a target language (or `auto`).
    Nemotron,
}

/// Configuration for [`ParakeetTranscriber`].
#[derive(Debug, Clone)]
pub struct ParakeetConfig {
    pub architecture: ParakeetArchitecture,
    /// Directory holding the model's ONNX graphs and tokenizer.
    pub model_dir: PathBuf,
    /// Language code (e.g. "de", "en") or "auto".
    pub language: String,
    /// ONNX Runtime intra-op threads (None = parakeet-rs default).
    pub threads: Option<usize>,
    /// How long the model can sit idle in memory before unloading. `None` = never unload.
    pub idle_unload_after: Option<Duration>,
}

/// A loaded model. Boxed: the engines are large and moved in and out of the slot.
enum Engine {
    Tdt(Box<ParakeetTDT>),
    Nemotron(Box<Nemotron>),
}

/// Parakeet TDT / Nemotron transcriber.
///
/// The engine needs `&mut` access, so it sits behind a mutex; it is unloaded
/// after the configured idle time and reloaded lazily on the next chunk.
pub struct ParakeetTranscriber {
    engine: Mutex<Option<Engine>>,
    config: ParakeetConfig,
    model_name: String,
    idle: IdleTracker,
}

impl std::fmt::Debug for ParakeetTranscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParakeetTranscriber")
            .field("config", &self.config)
            .field("model_name", &self.model_name)
            .finish_non_exhaustive()
    }
}

impl ParakeetTranscriber {
    /// Load the model using the system clock for idle tracking.
    ///
    /// # Errors
    /// `TranscriptionModelNotFound` if the model directory doesn't exist,
    /// `TranscriptionInferenceFailed` if ONNX Runtime can't load the model or
    /// Nemotron doesn't support the configured language.
    pub fn new(config: ParakeetConfig) -> Result<Self> {
        Self::new_with_clock(config, Arc::new(SystemClock))
    }

    /// Like [`new`](Self::new), with an injected clock for idle tracking.
    pub(crate) fn new_with_clock(config: ParakeetConfig, clock: Arc<dyn Clock>) -> Result<Self> {
        if !config.model_dir.is_dir() {
            return Err(VoicshError::TranscriptionModelNotFound {
                path: config.model_dir.display().to_string(),
            });
        }
        if config.architecture == ParakeetArchitecture::Tdt
            && config.language != defaults::AUTO_LANGUAGE
        {
            eprintln!(
                "voicsh: Parakeet TDT detects the spoken language itself; language '{}' is not applied.",
                config.language
            );
        }
        let model_name = config.model_dir.file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let engine = load_engine(&config)?;
        let idle = IdleTracker::new(config.idle_unload_after, clock);
        Ok(Self {
            engine: Mutex::new(Some(engine)),
            config,
            model_name,
            idle,
        })
    }

    /// Returns `true` if the model is currently loaded in memory.
    #[cfg(test)]
    #[allow(clippy::unwrap_used)] // Mutex poisoning means the owning thread already panicked; propagate.
    pub(crate) fn is_loaded(&self) -> bool {
        self.engine.lock().unwrap().is_some()
    }
}

impl Transcriber for ParakeetTranscriber {
    fn transcribe(&self, audio: &[i16]) -> Result<TranscriptionResult> {
        let samples = pcm_to_f32(audio);
        let mut guard =
            self.engine
                .lock()
                .map_err(|e| VoicshError::TranscriptionInferenceFailed {
                    message: format!("Failed to acquire model lock: {e}"),
                })?;
        let mut engine = match guard.take() {
            Some(engine) => engine,
            None => {
                eprintln!("voicsh: reloading model '{}'", self.model_name);
                load_engine(&self.config)?
            }
        };
        let result = engine.transcribe(samples, &self.config.language);
        *guard = Some(engine);
        drop(guard);
        self.idle.mark_used();
        result
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn try_unload_if_idle(&self) -> bool {
        let unloaded = self.idle.unload_if_idle(&self.engine);
        if unloaded {
            eprintln!(
                "voicsh: unloaded model '{}' after {:?} idle",
                self.model_name,
                self.idle.threshold().unwrap_or_default()
            );
        }
        unloaded
    }
}

impl Engine {
    fn transcribe(
        &mut self,
        samples: Vec<f32>,
        configured_language: &str,
    ) -> Result<TranscriptionResult> {
        match self {
            Engine::Tdt(model) => {
                let result = model
                    .transcribe_samples(samples, defaults::SAMPLE_RATE, 1, None)
                    .map_err(inference_error)?;
                Ok(TranscriptionResult::from_text(strip_unknown_token(
                    &result.text,
                )))
            }
            Engine::Nemotron(model) => {
                let tokens = model
                    .transcribe_audio_with_tokens(&samples)
                    .map_err(inference_error)?;
                Ok(nemotron_result(
                    tokens.iter().map(|t| (t.text.as_str(), t.logprob)),
                    configured_language,
                ))
            }
        }
    }
}

fn inference_error(error: parakeet_rs::Error) -> VoicshError {
    VoicshError::TranscriptionInferenceFailed {
        message: format!("ONNX inference failed: {error}"),
    }
}

fn load_engine(config: &ParakeetConfig) -> Result<Engine> {
    let execution = execution_config(config.threads)?;
    let load_error = |error: parakeet_rs::Error| VoicshError::TranscriptionInferenceFailed {
        message: format!(
            "Failed to load model from {}: {error}",
            config.model_dir.display()
        ),
    };
    match config.architecture {
        ParakeetArchitecture::Tdt => {
            ParakeetTDT::from_pretrained(&config.model_dir, Some(execution))
                .map(|model| Engine::Tdt(Box::new(model)))
                .map_err(load_error)
        }
        ParakeetArchitecture::Nemotron => {
            let mut model = Nemotron::from_pretrained(&config.model_dir, Some(execution))
                .map_err(load_error)?;
            model.set_target_lang(&config.language).map_err(|e| {
                VoicshError::TranscriptionInferenceFailed {
                    message: format!(
                        "Nemotron cannot transcribe language '{}': {e}\n\
                         Set a supported language, e.g.: voicsh config set stt.language auto",
                        config.language
                    ),
                }
            })?;
            Ok(Engine::Nemotron(Box::new(model)))
        }
    }
}

fn execution_config(threads: Option<usize>) -> Result<ExecutionConfig> {
    let config = ExecutionConfig::new().with_environment(ort_environment()?);
    Ok(match threads {
        Some(threads) => config.with_intra_threads(threads),
        None => config,
    })
}

/// The process-wide ONNX Runtime environment.
///
/// ONNX Runtime allows one environment per process, so it is created once and
/// shared by every model load (including reloads after idle unloading).
fn ort_environment() -> Result<Environment> {
    static ENVIRONMENT: OnceLock<std::result::Result<Environment, String>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            parakeet_rs::ort::init()
                .with_name("voicsh")
                .with_telemetry(false)
                .build()
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(|message| VoicshError::TranscriptionInferenceFailed {
            message: format!("Failed to initialise ONNX Runtime: {message}"),
        })
}

/// Build a result from Nemotron's `(piece, log-probability)` tokens.
///
/// The multilingual model emits inline language tags (`<de-DE>`); the first
/// one becomes the result language (primary subtag, e.g. "de"). Without a
/// tag, a non-auto configured language is reported. Confidence is the mean
/// token probability, as for whisper.
fn nemotron_result<'a>(
    tokens: impl IntoIterator<Item = (&'a str, f32)>,
    configured_language: &str,
) -> TranscriptionResult {
    let mut text = String::new();
    let mut detected_language = None;
    let mut token_probabilities = Vec::new();
    for (piece, logprob) in tokens {
        if let Some(tag) = language_tag(piece) {
            detected_language.get_or_insert(tag);
            continue;
        }
        text.push_str(piece);
        token_probabilities.push(TokenProbability {
            token: piece.to_string(),
            probability: logprob.exp(),
        });
    }
    let confidence = if token_probabilities.is_empty() {
        0.0
    } else {
        token_probabilities
            .iter()
            .map(|t| t.probability)
            .sum::<f32>()
            / token_probabilities.len() as f32
    };
    let language = match detected_language {
        Some(tag) => tag.to_string(),
        None if configured_language == defaults::AUTO_LANGUAGE => String::new(),
        None => configured_language.to_string(),
    };
    TranscriptionResult {
        text: strip_unknown_token(&text),
        language,
        confidence,
        token_probabilities,
    }
}

/// SentencePiece out-of-vocabulary marker; Parakeet Ultra emits it where its
/// training text had parentheses.
const UNKNOWN_TOKEN: &str = "<unk>";

/// Remove every `<unk>` marker (also when glued to words), collapse the
/// whitespace runs this leaves behind, and trim.
fn strip_unknown_token(text: &str) -> String {
    text.replace(UNKNOWN_TOKEN, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Primary language subtag of a Nemotron language-tag piece: `<de-DE>` / `<de>` → "de".
fn language_tag(piece: &str) -> Option<&str> {
    let inner = piece.trim().strip_prefix('<')?.strip_suffix('>')?;
    let (primary, region) = match inner.split_once('-') {
        Some((primary, region)) => (primary, Some(region)),
        None => (inner, None),
    };
    let is_primary = primary.len() == 2 && primary.bytes().all(|b| b.is_ascii_lowercase());
    let is_region = region
        .is_none_or(|region| region.len() == 2 && region.bytes().all(|b| b.is_ascii_uppercase()));
    (is_primary && is_region).then_some(primary)
}

#[cfg(test)]
fn config(architecture: ParakeetArchitecture, model_dir: PathBuf) -> ParakeetConfig {
    ParakeetConfig {
        architecture,
        model_dir,
        language: defaults::AUTO_LANGUAGE.to_string(),
        threads: Some(4),
        idle_unload_after: Some(Duration::from_secs(5)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_unknown_token_removes_glued_markers() {
        assert_eq!(
            strip_unknown_token("hello <unk>world<unk> x"),
            "hello world x"
        );
    }

    #[test]
    fn test_strip_unknown_token_leaves_clean_text_unchanged() {
        assert_eq!(strip_unknown_token("hello world"), "hello world");
    }

    #[test]
    fn test_strip_unknown_token_only_marker_yields_empty() {
        assert_eq!(strip_unknown_token(" <unk> "), "");
    }

    #[test]
    fn test_strip_unknown_token_collapses_double_spaces() {
        assert_eq!(strip_unknown_token("a <unk> b"), "a b");
    }

    #[test]
    fn test_new_fails_with_model_not_found_for_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("parakeet-tdt-0.6b-v3");

        let error = ParakeetTranscriber::new(config(ParakeetArchitecture::Tdt, missing.clone()))
            .unwrap_err();

        match error {
            VoicshError::TranscriptionModelNotFound { path } => {
                assert_eq!(path, missing.display().to_string());
            }
            other => panic!("expected TranscriptionModelNotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_new_fails_with_load_error_naming_directory_for_empty_tdt_dir() {
        let dir = tempfile::tempdir().unwrap();

        let error =
            ParakeetTranscriber::new(config(ParakeetArchitecture::Tdt, dir.path().to_path_buf()))
                .unwrap_err();

        match error {
            VoicshError::TranscriptionInferenceFailed { message } => {
                let expected = format!("Failed to load model from {}", dir.path().display());
                assert!(message.starts_with(&expected), "message: {message}");
            }
            other => panic!("expected TranscriptionInferenceFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_new_fails_with_load_error_naming_directory_for_empty_nemotron_dir() {
        let dir = tempfile::tempdir().unwrap();

        let error = ParakeetTranscriber::new(config(
            ParakeetArchitecture::Nemotron,
            dir.path().to_path_buf(),
        ))
        .unwrap_err();

        match error {
            VoicshError::TranscriptionInferenceFailed { message } => {
                let expected = format!("Failed to load model from {}", dir.path().display());
                assert!(message.starts_with(&expected), "message: {message}");
            }
            other => panic!("expected TranscriptionInferenceFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_ort_environment_is_shared_across_calls() {
        use parakeet_rs::ort::AsPointer;
        // ONNX Runtime refuses a second environment per process; both calls
        // must succeed by returning the same shared environment.
        let first = ort_environment().unwrap();
        let second = ort_environment().unwrap();
        assert_eq!(first.ptr(), second.ptr());
    }

    #[test]
    fn test_language_tag_extracts_primary_subtag() {
        assert_eq!(language_tag("<de-DE>"), Some("de"));
        assert_eq!(language_tag("<en>"), Some("en"));
        assert_eq!(language_tag(" <fr-CA>"), Some("fr"));
    }

    #[test]
    fn test_language_tag_rejects_non_tags() {
        assert_eq!(language_tag(" Hallo"), None);
        assert_eq!(language_tag("<unk>"), None);
        assert_eq!(language_tag("<de-de>"), None);
        assert_eq!(language_tag("<de->"), None);
        assert_eq!(language_tag("<>"), None);
        assert_eq!(language_tag("<"), None);
    }

    #[test]
    fn test_nemotron_result_strips_tags_and_reports_detected_language() {
        let tokens = [
            ("<de-DE>", 0.0),
            ("Hallo", (0.5f32).ln()),
            (" Welt", (1.0f32).ln()),
        ];

        let result = nemotron_result(tokens, defaults::AUTO_LANGUAGE);

        assert_eq!(result.text, "Hallo Welt");
        assert_eq!(result.language, "de");
        assert!(
            (result.confidence - 0.75).abs() < 1e-6,
            "{}",
            result.confidence
        );
        assert_eq!(
            result.token_probabilities,
            vec![
                TokenProbability {
                    token: "Hallo".to_string(),
                    probability: 0.5,
                },
                TokenProbability {
                    token: " Welt".to_string(),
                    probability: 1.0,
                },
            ]
        );
    }

    #[test]
    fn test_nemotron_result_keeps_first_tag_for_mixed_language() {
        let tokens = [
            ("<de-DE>", 0.0),
            ("Ich", 0.0),
            ("<en-US>", 0.0),
            (" deploy", 0.0),
        ];

        let result = nemotron_result(tokens, defaults::AUTO_LANGUAGE);

        assert_eq!(result.text, "Ich deploy");
        assert_eq!(result.language, "de");
    }

    #[test]
    fn test_nemotron_result_without_tag_reports_configured_language() {
        let result = nemotron_result([(" Guten", 0.0), (" Tag", 0.0)], "de");

        assert_eq!(result.text, "Guten Tag");
        assert_eq!(result.language, "de");
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn test_nemotron_result_without_tag_in_auto_mode_reports_unknown_language() {
        let result = nemotron_result([("Hi", 0.0)], defaults::AUTO_LANGUAGE);
        assert_eq!(result.language, "");
    }

    #[test]
    fn test_nemotron_result_without_tokens_is_empty_with_zero_confidence() {
        let result = nemotron_result([], defaults::AUTO_LANGUAGE);

        assert_eq!(result.text, "");
        assert_eq!(result.confidence, 0.0);
        assert_eq!(result.token_probabilities, Vec::new());
    }

    #[test]
    fn test_parakeet_transcriber_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ParakeetTranscriber>();
    }
}

/// Real-model tests. Ignored by default: they need the models installed
/// (`voicsh models install parakeet-tdt-0.6b-v3` and
/// `voicsh models install nemotron-3.5-asr-0.6b`) and take seconds each.
/// Run with:
/// `cargo test --lib --features parakeet stt::parakeet -- --ignored`
#[cfg(all(test, feature = "model-download"))]
mod real_model_tests {
    use super::*;
    use crate::audio::vad::MockClock;

    fn installed_model_dir(name: &str) -> PathBuf {
        let dir = crate::models::download::model_path(name);
        assert!(
            dir.is_dir(),
            "model '{name}' not installed at {}; run: voicsh models install {name}",
            dir.display()
        );
        dir
    }

    fn quick_brown_fox() -> Vec<i16> {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick_brown_fox.wav");
        let data = std::fs::read(&path).unwrap();
        crate::audio::wav::WavAudioSource::from_reader(Box::new(std::io::Cursor::new(data)))
            .unwrap()
            .into_samples()
    }

    fn assert_quick_brown_fox(text: &str) {
        let text = text.to_lowercase();
        for word in ["quick", "brown", "fox", "lazy", "dog"] {
            assert!(text.contains(word), "expected '{word}' in '{text}'");
        }
    }

    /// Transcribes the English fixture with the installed Parakeet TDT model.
    #[test]
    #[ignore = "needs installed parakeet-tdt-0.6b-v3 model"]
    fn test_tdt_transcribes_known_speech() {
        let dir = installed_model_dir("parakeet-tdt-0.6b-v3");
        let transcriber = ParakeetTranscriber::new(config(ParakeetArchitecture::Tdt, dir)).unwrap();

        let result = transcriber.transcribe(&quick_brown_fox()).unwrap();

        assert_quick_brown_fox(&result.text);
        assert_eq!(transcriber.model_name(), "parakeet-tdt-0.6b-v3");
    }

    /// Transcribes the English fixture with the installed Nemotron model and
    /// checks the detected language and token confidence.
    #[test]
    #[ignore = "needs installed nemotron-3.5-asr-0.6b model"]
    fn test_nemotron_transcribes_known_speech() {
        let dir = installed_model_dir("nemotron-3.5-asr-0.6b");
        let transcriber =
            ParakeetTranscriber::new(config(ParakeetArchitecture::Nemotron, dir)).unwrap();

        let result = transcriber.transcribe(&quick_brown_fox()).unwrap();

        assert_quick_brown_fox(&result.text);
        assert_eq!(result.language, "en");
        assert!(result.confidence > 0.5, "confidence {}", result.confidence);
    }

    /// Rejects a language Nemotron has no prompt for, with a hint to fix the config.
    #[test]
    #[ignore = "needs installed nemotron-3.5-asr-0.6b model"]
    fn test_nemotron_rejects_unsupported_language() {
        let mut config = config(
            ParakeetArchitecture::Nemotron,
            installed_model_dir("nemotron-3.5-asr-0.6b"),
        );
        config.language = "xx".to_string();

        let error = ParakeetTranscriber::new(config).unwrap_err();

        match error {
            VoicshError::TranscriptionInferenceFailed { message } => {
                assert!(
                    message.starts_with("Nemotron cannot transcribe language 'xx'"),
                    "message: {message}"
                );
            }
            other => panic!("expected TranscriptionInferenceFailed, got {other:?}"),
        }
    }

    /// Unloads after the idle threshold and reloads lazily on the next chunk.
    #[test]
    #[ignore = "needs installed parakeet-tdt-0.6b-v3 model"]
    fn test_tdt_unloads_when_idle_and_reloads_on_transcribe() {
        let clock = Arc::new(MockClock::new());
        let dir = installed_model_dir("parakeet-tdt-0.6b-v3");
        let transcriber = ParakeetTranscriber::new_with_clock(
            config(ParakeetArchitecture::Tdt, dir),
            clock.clone(),
        )
        .unwrap();

        clock.advance(Duration::from_secs(6));
        assert_eq!(transcriber.try_unload_if_idle(), true);
        assert_eq!(transcriber.is_loaded(), false);

        let result = transcriber.transcribe(&quick_brown_fox()).unwrap();

        assert_quick_brown_fox(&result.text);
        assert_eq!(transcriber.is_loaded(), true);
    }
}
