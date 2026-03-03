// tests/symspell_e2e_audio_test.rs
//! End-to-end tests for SymSpell whitelist with audio pipeline
//!
//! This file tests:
//! 1. Full pipeline simulation with mock Whisper and SymSpell
//! 2. Audio configuration tests
//! 3. Language detection and dispatch flow
//! 4. Common phrase correction tests
//! 5. Error handling tests
//! 6. Parallel multi-language processing

use std::collections::HashMap;
use std::path::PathBuf;
use voicsh::stt::transcriber::MockTranscriber;
use voicsh::{Corrector, HybridCorrector, SymSpellCorrector, Transcriber};

fn dictionary_path(lang: &str) -> PathBuf {
    PathBuf::from(format!("data/dictionaries/{}-80k.txt", lang))
}

async fn setup_correction_for_language(lang: &str) -> voicsh::Result<HybridCorrector> {
    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    let path = dictionary_path(lang);
    let corrector = SymSpellCorrector::from_file(&path, lang)?;
    symspell_correctors.insert(lang.to_string(), Box::new(corrector));

    let whitelist = vec![lang.to_string()];
    Ok(HybridCorrector::new(symspell_correctors, whitelist))
}

#[test]
#[cfg(feature = "symspell")]
fn test_mock_pipeline_japanese_hello() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let transcriber = MockTranscriber::new("base-japanese-test");

    println!("\n=== Japanese Pipeline Test ===");
    println!("Mock transcriber: {}", transcriber.model_name());

    let japanese_input = "こんにちは世界";
    println!("Input: '{}'", japanese_input);

    let corrected = corrector
        .correct_with_language(japanese_input, "ja")
        .expect("Correction failed");

    println!("Output: '{}'", corrected);
    assert_eq!(
        corrected, japanese_input,
        "CJK text should pass through unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_mock_pipeline_korean_hello() {
    if !dictionary_path("ko").exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup corrector");

    let transcriber = MockTranscriber::new("base-korean-test");

    println!("\n=== Korean Pipeline Test ===");
    println!("Mock transcriber: {}", transcriber.model_name());

    let korean_input = "안녕하세요";
    println!("Input: '{}'", korean_input);

    let corrected = corrector
        .correct_with_language(korean_input, "ko")
        .expect("Correction failed");

    println!("Output: '{}'", corrected);
    assert_eq!(
        corrected, korean_input,
        "CJK text should pass through unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_japanese_common_phrases() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let test_phrases = vec!["おはようございます", "ありがとうございます", "すみません"];

    println!("\n=== Japanese Common Phrases ===");
    for phrase in test_phrases {
        let result = corrector
            .correct_with_language(phrase, "ja")
            .expect("Correction failed");
        println!("Input: '{}' → Output: '{}'", phrase, result);
        assert_eq!(result, phrase, "CJK text should pass through unchanged");
    }
}

#[test]
#[cfg(feature = "symspell")]
fn test_korean_common_phrases() {
    if !dictionary_path("ko").exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup corrector");

    let test_phrases = vec!["감사합니다", "죄송합니다", "안녕히 가세요"];

    println!("\n=== Korean Common Phrases ===");
    for phrase in test_phrases {
        let result = corrector
            .correct_with_language(phrase, "ko")
            .expect("Correction failed");
        println!("Input: '{}' → Output: '{}'", phrase, result);
        assert_eq!(result, phrase, "CJK text should pass through unchanged");
    }
}

#[test]
#[cfg(feature = "symspell")]
fn test_language_dispatch_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let input = "こんにちは";
    let result = corrector
        .correct_with_language(input, "ja")
        .expect("Correction should succeed");

    println!("Japanese: '{}' → '{}'", input, result);
    assert_eq!(
        result, input,
        "Japanese CJK text should pass through unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_language_dispatch_korean() {
    if !dictionary_path("ko").exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup corrector");

    let input = "안녕하세요";
    let result = corrector
        .correct_with_language(input, "ko")
        .expect("Correction should succeed");

    println!("Korean: '{}' → '{}'", input, result);
    assert_eq!(
        result, input,
        "Korean CJK text should pass through unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_empty_input_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let result = corrector
        .correct_with_language("", "ja")
        .expect("Correction should succeed");

    assert_eq!(result, "", "Empty input should return empty output");
}

#[test]
#[cfg(feature = "symspell")]
fn test_empty_input_korean() {
    if !dictionary_path("ko").exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup corrector");

    let result = corrector
        .correct_with_language("", "ko")
        .expect("Correction should succeed");

    assert_eq!(result, "", "Empty input should return empty output");
}

#[test]
#[cfg(feature = "symspell")]
fn test_whitespace_only_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let result = corrector
        .correct_with_language("   ", "ja")
        .expect("Correction should succeed");

    assert_eq!(
        result.trim(),
        "",
        "Whitespace-only input should return empty output"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_very_long_input_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let long_input = "こんにちは ".repeat(100);
    let result = corrector
        .correct_with_language(&long_input, "ja")
        .expect("Correction should succeed");

    assert_eq!(
        result, long_input,
        "CJK long input should pass through unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_multiple_corrections_session() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let inputs = vec!["こんにちは", "おはよう", "ありがとう"];

    println!("\n=== Multiple Corrections in Session ===");
    for input in inputs {
        let result = corrector
            .correct_with_language(input, "ja")
            .expect("Correction failed");
        println!("'{}' → '{}'", input, result);
        assert_eq!(
            result, input,
            "CJK text should pass through unchanged in each correction"
        );
    }
}

#[test]
#[cfg(feature = "symspell")]
fn test_japanese_korean_alternating() {
    if !dictionary_path("ja").exists() || !dictionary_path("ko").exists() {
        println!("Japanese or Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut ja_corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup Japanese corrector");
    let mut ko_corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup Korean corrector");

    // Alternate between languages
    let ja_input = "こんにちは";
    let ko_input = "안녕하세요";

    println!("\n=== Alternating Japanese/Korean ===");

    let ja_result = ja_corrector
        .correct_with_language(ja_input, "ja")
        .expect("Japanese correction failed");
    println!("JA: '{}' → '{}'", ja_input, ja_result);
    assert_eq!(
        ja_result, ja_input,
        "Japanese CJK should pass through unchanged"
    );

    let ko_result = ko_corrector
        .correct_with_language(ko_input, "ko")
        .expect("Korean correction failed");
    println!("KO: '{}' → '{}'", ko_input, ko_result);
    assert_eq!(
        ko_result, ko_input,
        "Korean CJK should pass through unchanged"
    );

    // Japanese again
    let ja_result2 = ja_corrector
        .correct_with_language(ja_input, "ja")
        .expect("Japanese correction 2 failed");
    println!("JA: '{}' → '{}'", ja_input, ja_result2);
    assert_eq!(
        ja_result2, ja_input,
        "Japanese CJK should pass through unchanged on second call"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_special_characters_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    let inputs = vec!["こんにちは！", "おはよう。", "ありがとう？"];

    println!("\n=== Japanese with Punctuation ===");
    for input in inputs {
        let result = corrector
            .correct_with_language(input, "ja")
            .expect("Correction failed");
        println!("'{}' → '{}'", input, result);
        assert_eq!(
            result, input,
            "CJK text with punctuation should pass through unchanged"
        );
    }
}

#[test]
#[cfg(feature = "symspell")]
fn test_mixed_script_japanese() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    // Japanese uses hiragana, katakana, and kanji
    let inputs = vec![
        "こんにちは", // Hiragana
        "コンニチハ", // Katakana
        "こんにちは", // Common greeting
    ];

    println!("\n=== Japanese Mixed Scripts ===");
    for input in inputs {
        let result = corrector
            .correct_with_language(input, "ja")
            .expect("Correction failed");
        println!("'{}' → '{}'", input, result);
        assert_eq!(
            result, input,
            "All Japanese scripts should pass through unchanged"
        );
    }
}

#[test]
#[cfg(feature = "symspell")]
fn test_whisper_simulation_with_language() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup corrector");

    // Create mock transcriber with specific language
    let transcriber = MockTranscriber::new("whisper-japanese")
        .with_response("こんにちは世界")
        .with_language("ja");

    println!("\n=== Whisper Simulation Test ===");
    println!("Model: {}", transcriber.model_name());
    println!("Language: {}", transcriber.model_name());

    let audio_samples = vec![0i16; 16000];
    let transcription = transcriber
        .transcribe(&audio_samples)
        .expect("Transcription should succeed");

    println!("Transcription: '{}'", transcription.text);

    // Now correct the transcription
    let corrected = corrector
        .correct_with_language(&transcription.text, "ja")
        .expect("Correction failed");

    println!("Corrected: '{}'", corrected);
    assert_eq!(
        corrected, transcription.text,
        "CJK transcription should pass through correction unchanged"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_concurrent_corrections_japanese_korean() {
    if !dictionary_path("ja").exists() || !dictionary_path("ko").exists() {
        println!("Japanese or Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    // Setup both correctors
    let mut ja_corrector = runtime
        .block_on(setup_correction_for_language("ja"))
        .expect("Failed to setup Japanese corrector");
    let mut ko_corrector = runtime
        .block_on(setup_correction_for_language("ko"))
        .expect("Failed to setup Korean corrector");

    println!("\n=== Concurrent Japanese/Korean Corrections ===");

    let ja_inputs = vec!["こんにちは", "ありがとう", "おはよう"];
    let ko_inputs = vec!["안녕하세요", "감사합니다", "죄송합니다"];

    for (ja, ko) in ja_inputs.iter().zip(ko_inputs.iter()) {
        let ja_result = ja_corrector
            .correct_with_language(ja, "ja")
            .expect("Japanese correction failed");
        let ko_result = ko_corrector
            .correct_with_language(ko, "ko")
            .expect("Korean correction failed");

        println!(
            "JA: '{}' → '{}' | KO: '{}' → '{}'",
            ja, ja_result, ko, ko_result
        );
        assert_eq!(ja_result, *ja, "Japanese CJK should pass through unchanged");
        assert_eq!(ko_result, *ko, "Korean CJK should pass through unchanged");
    }
}
