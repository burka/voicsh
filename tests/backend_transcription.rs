#![cfg(feature = "whisper")]

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use voicsh::audio::wav::WavAudioSource;
use voicsh::models::download::{find_any_installed_model, model_path};
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

fn find_model() -> Option<PathBuf> {
    let model_name = find_any_installed_model()?;
    let path = model_path(&model_name);
    if path.exists() {
        Some(path)
    } else {
        eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║  NO WHISPER MODEL FOUND — SKIPPING BACKEND TESTS             ║");
        eprintln!("║                                                              ║");
        eprintln!("║  Install a model with:                                       ║");
        eprintln!("║    cargo run -- models install tiny.en                       ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝\n");
        None
    }
}

fn language_for_model(path: &Path) -> &'static str {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.contains(".en"))
        .unwrap_or(false)
    {
        "en"
    } else {
        "auto"
    }
}

fn compiled_backend() -> &'static str {
    if cfg!(feature = "cuda") {
        "CUDA"
    } else if cfg!(feature = "vulkan") {
        "Vulkan"
    } else if cfg!(feature = "hipblas") {
        "HIP/ROCm"
    } else if cfg!(feature = "openblas") {
        "OpenBLAS"
    } else {
        "CPU"
    }
}

fn load_fixture() -> Vec<i16> {
    let fixture_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick_brown_fox.wav");
    let file = File::open(fixture_path).expect("Failed to open fixture");
    let reader = BufReader::new(file);
    let wav_source =
        WavAudioSource::from_reader(Box::new(reader)).expect("Failed to parse WAV fixture");
    wav_source.into_samples()
}

#[test]
fn test_cpu_transcribes_known_speech() {
    let Some(model_path) = find_model() else {
        return;
    };

    let config = WhisperConfig {
        model_path: model_path.clone(),
        language: language_for_model(&model_path).to_string(),
        threads: Some(4),
        use_gpu: false,
    };

    let transcriber = match WhisperTranscriber::new(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[CPU] Failed to create transcriber: {e} — skipping");
            return;
        }
    };

    eprintln!(
        "[CPU] Backend: {}, Model: {}",
        compiled_backend(),
        transcriber.model_name()
    );

    let audio = load_fixture();
    let result = transcriber
        .transcribe(&audio)
        .expect("CPU transcription failed");

    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("quick"),
        "Expected 'quick' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("brown"),
        "Expected 'brown' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("fox"),
        "Expected 'fox' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("lazy"),
        "Expected 'lazy' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("dog"),
        "Expected 'dog' in transcription, got: {}",
        result.text
    );
    assert!(
        result.confidence > 0.5,
        "Expected confidence > 0.5, got: {}",
        result.confidence
    );

    eprintln!("[CPU] Transcription: \"{}\"", result.text);
    eprintln!("[CPU] Confidence: {:.2}", result.confidence);
}

#[test]
fn test_gpu_transcribes_known_speech() {
    let Some(model_path) = find_model() else {
        return;
    };

    let config = WhisperConfig {
        model_path: model_path.clone(),
        language: language_for_model(&model_path).to_string(),
        threads: Some(4),
        use_gpu: true,
    };

    let transcriber = match WhisperTranscriber::new(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "[{} GPU] Backend unavailable: {e} — skipping",
                compiled_backend()
            );
            return;
        }
    };

    eprintln!(
        "[{} GPU] Model: {}",
        compiled_backend(),
        transcriber.model_name()
    );

    let audio = load_fixture();
    let result = transcriber
        .transcribe(&audio)
        .expect("GPU transcription failed");

    let text_lower = result.text.to_lowercase();
    assert!(
        text_lower.contains("quick"),
        "Expected 'quick' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("brown"),
        "Expected 'brown' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("fox"),
        "Expected 'fox' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("lazy"),
        "Expected 'lazy' in transcription, got: {}",
        result.text
    );
    assert!(
        text_lower.contains("dog"),
        "Expected 'dog' in transcription, got: {}",
        result.text
    );
    assert!(
        result.confidence > 0.5,
        "Expected confidence > 0.5, got: {}",
        result.confidence
    );

    eprintln!(
        "[{} GPU] Transcription: \"{}\"",
        compiled_backend(),
        result.text
    );
    eprintln!(
        "[{} GPU] Confidence: {:.2}",
        compiled_backend(),
        result.confidence
    );
}

#[test]
fn test_cpu_transcribes_silence() {
    let Some(model_path) = find_model() else {
        return;
    };

    let config = WhisperConfig {
        model_path: model_path.clone(),
        language: language_for_model(&model_path).to_string(),
        threads: Some(4),
        use_gpu: false,
    };

    let transcriber = match WhisperTranscriber::new(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[CPU] Failed to create transcriber: {e} — skipping");
            return;
        }
    };

    eprintln!("[CPU] Testing silence transcription");

    // 1 second of silence at 16kHz
    let silence = vec![0i16; 16000];
    let result = transcriber
        .transcribe(&silence)
        .expect("CPU silence transcription failed");

    eprintln!("[CPU] Silence result: \"{}\"", result.text);
    eprintln!("[CPU] Confidence: {:.2}", result.confidence);

    // Silence should produce empty or very short output
    assert!(
        result.text.len() < 50,
        "Expected short/empty text for silence, got {} chars: {}",
        result.text.len(),
        result.text
    );
}

#[test]
fn test_gpu_transcribes_silence() {
    let Some(model_path) = find_model() else {
        return;
    };

    let config = WhisperConfig {
        model_path: model_path.clone(),
        language: language_for_model(&model_path).to_string(),
        threads: Some(4),
        use_gpu: true,
    };

    let transcriber = match WhisperTranscriber::new(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "[{} GPU] Backend unavailable: {e} — skipping",
                compiled_backend()
            );
            return;
        }
    };

    eprintln!("[{} GPU] Testing silence transcription", compiled_backend());

    // 1 second of silence at 16kHz
    let silence = vec![0i16; 16000];
    let result = transcriber
        .transcribe(&silence)
        .expect("GPU silence transcription failed");

    eprintln!(
        "[{} GPU] Silence result: \"{}\"",
        compiled_backend(),
        result.text
    );
    eprintln!(
        "[{} GPU] Confidence: {:.2}",
        compiled_backend(),
        result.confidence
    );

    // Silence should produce empty or very short output
    assert!(
        result.text.len() < 50,
        "Expected short/empty text for silence, got {} chars: {}",
        result.text.len(),
        result.text
    );
}
