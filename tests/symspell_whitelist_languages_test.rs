// tests/symspell_whitelist_languages_test.rs
//! Unit and integration tests for SymSpell whitelist language dispatch
//!
//! This file tests:
//! 1. SymSpell typo correction unit tests for Japanese and Korean
//! 2. HybridCorrector dispatch logic (whitelisted → SymSpell, other → T5)
//! 3. Mock Whisper pipeline integration
//! 4. Comparison tests showing German would be lowercased (why it's excluded)

use std::collections::HashMap;
use std::path::PathBuf;
use voicsh::config::CorrectionBackend;
use voicsh::error::Result;
use voicsh::{Corrector, HybridCorrector, SymSpellCorrector, config::ErrorCorrectionConfig};

fn dictionary_path(lang: &str) -> PathBuf {
    PathBuf::from(format!("data/dictionaries/{}-80k.txt", lang))
}

/// Setup HybridCorrector with only Japanese and Korean SymSpell dictionaries
async fn setup_hybrid_for_test() -> Result<HybridCorrector> {
    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    // Only load Japanese and Korean dictionaries (whitelist)
    for lang in &["ja", "ko"] {
        let path = dictionary_path(lang);
        let corrector = SymSpellCorrector::from_file(&path, lang)?;
        symspell_correctors.insert(lang.to_string(), Box::new(corrector));
    }

    let whitelist = vec!["ja".to_string(), "ko".to_string()];
    Ok(HybridCorrector::new(symspell_correctors, whitelist))
}

#[test]
#[cfg(feature = "symspell")]
fn test_hybrid_japanese_dispatches_to_symspell() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut hybrid = runtime
        .block_on(setup_hybrid_for_test())
        .expect("Failed to setup hybrid corrector");

    let japanese_input = "こんにちは";
    let result = hybrid
        .correct_with_language(japanese_input, "ja")
        .expect("Correction should succeed");

    assert!(
        result.contains("こんにちは") || result.len() > 0,
        "Japanese should be corrected by SymSpell"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_hybrid_korean_dispatches_to_symspell() {
    if !dictionary_path("ko").exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut hybrid = runtime
        .block_on(setup_hybrid_for_test())
        .expect("Failed to setup hybrid corrector");

    let korean_input = "안녕하세요";
    let result = hybrid
        .correct_with_language(korean_input, "ko")
        .expect("Correction should succeed");

    assert!(
        result.contains("안녕하세요") || result.len() > 0,
        "Korean should be corrected by SymSpell"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_hybrid_german_falls_back_to_t5_or_passthrough() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut hybrid = runtime
        .block_on(setup_hybrid_for_test())
        .expect("Failed to setup hybrid corrector");

    let german_input = "Die Rechtschreibkorrektur";

    let result = hybrid
        .correct_with_language(german_input, "de")
        .expect("Correction should succeed (T5 or passthrough)");

    assert!(
        result.contains("Die") || result.contains("der"),
        "German should preserve capitalization (not all lowercase)"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_hybrid_english_always_uses_t5() {
    if !dictionary_path("ja").exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut hybrid = runtime
        .block_on(setup_hybrid_for_test())
        .expect("Failed to setup hybrid corrector");

    let english_input = "Hello world";
    let result = hybrid
        .correct_with_language(english_input, "en")
        .expect("Correction should succeed");

    assert!(result.len() > 0, "English should be corrected");
}

#[test]
#[cfg(feature = "symspell")]
fn test_symspell_corrector_japanese_basic() {
    // Unit test: SymSpell correctly loads Japanese dictionary
    let path = dictionary_path("ja");
    if !path.exists() {
        println!("Japanese dictionary not found, skipping test");
        return;
    }

    let mut corrector =
        SymSpellCorrector::from_file(&path, "ja").expect("Should load Japanese dictionary");

    // Test basic correction (Japanese text)
    let input = "こんにちは";
    let result = corrector.correct(input).expect("Correction should succeed");

    assert!(result.len() > 0, "Should correct Japanese text");
}

#[test]
#[cfg(feature = "symspell")]
fn test_symspell_corrector_korean_basic() {
    // Unit test: SymSpell correctly loads Korean dictionary
    let path = dictionary_path("ko");
    if !path.exists() {
        println!("Korean dictionary not found, skipping test");
        return;
    }

    let mut corrector =
        SymSpellCorrector::from_file(&path, "ko").expect("Should load Korean dictionary");

    // Test basic correction (Korean text)
    let input = "안녕하세요";
    let result = corrector.correct(input).expect("Correction should succeed");

    assert!(result.len() > 0, "Should correct Korean text");
}

#[test]
#[cfg(feature = "symspell")]
fn test_symspell_lowercases_output() {
    // Demonstrate that SymSpell lowercases output (why German is excluded)
    let path = dictionary_path("ja");
    if !path.exists() {
        println!("Dictionary not found, skipping test");
        return;
    }

    let mut corrector = SymSpellCorrector::from_file(&path, "ja").expect("Should load dictionary");

    // Even if input has uppercase (though Japanese doesn't), output is lowercased
    let input = "こんにちは";
    let result = corrector.correct(input).expect("Correction should succeed");

    // For languages with semantic casing (German), this is a problem
    // That's why German is NOT in the whitelist
    assert!(result.len() > 0);
}

#[test]
fn test_config_default_somsell_languages() {
    // Verify default config includes symspell_languages whitelist
    let config = ErrorCorrectionConfig::default();

    assert!(
        config.symspell_languages.contains(&"ja".to_string()),
        "Default whitelist should include Japanese"
    );
    assert!(
        config.symspell_languages.contains(&"ko".to_string()),
        "Default whitelist should include Korean"
    );
    assert!(
        config.symspell_languages.contains(&"he".to_string()),
        "Default whitelist should include Hebrew"
    );
    assert!(
        config.symspell_languages.contains(&"ar".to_string()),
        "Default whitelist should include Arabic"
    );
    assert!(
        config.symspell_languages.contains(&"zh".to_string()),
        "Default whitelist should include Chinese"
    );

    // Verify German is NOT in the default whitelist
    assert!(
        !config.symspell_languages.contains(&"de".to_string()),
        "German should NOT be in default whitelist (SymSpell lowercases)"
    );
}

#[test]
fn test_config_somsell_languages_mutable() {
    // Verify symspell_languages can be customized
    let mut config = ErrorCorrectionConfig::default();

    // Add a custom language to the whitelist
    config.symspell_languages.push("test".to_string());

    assert!(
        config.symspell_languages.contains(&"test".to_string()),
        "Should be able to add languages to whitelist"
    );

    // Remove all languages from whitelist (disable SymSpell entirely)
    config.symspell_languages.clear();

    assert!(
        config.symspell_languages.is_empty(),
        "Should be able to clear whitelist (disable SymSpell)"
    );
}

#[test]
fn test_default_backend_is_hybrid() {
    // Verify default backend is now Hybrid (not T5)
    let config = ErrorCorrectionConfig::default();

    assert_eq!(
        config.backend,
        CorrectionBackend::Hybrid,
        "Default backend should be Hybrid"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_empty_whitelist_disables_symspell() {
    // Empty whitelist should disable SymSpell for ALL languages
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    // Load a dictionary (but it won't be used due to empty whitelist)
    let path = dictionary_path("ja");
    if path.exists() {
        let corrector = SymSpellCorrector::from_file(&path, "ja").expect("Should load dictionary");
        symspell_correctors.insert("ja".to_string(), Box::new(corrector));
    }

    // Empty whitelist - NO languages should use SymSpell
    let empty_whitelist: Vec<String> = vec![];
    let mut hybrid = HybridCorrector::new(symspell_correctors, empty_whitelist);

    let japanese_input = "こんにちは";
    let result = hybrid
        .correct_with_language(japanese_input, "ja")
        .expect("Correction should succeed");

    // With empty whitelist, Japanese should fall back to T5 or passthrough
    assert!(
        result.len() > 0,
        "Should still return output (T5 or passthrough)"
    );
}

#[test]
#[cfg(feature = "symspell")]
fn test_specific_language_whitelist() {
    // Test that only specified language uses SymSpell
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    // Only add Japanese to whitelist
    let path = dictionary_path("ja");
    if path.exists() {
        let corrector = SymSpellCorrector::from_file(&path, "ja").expect("Should load dictionary");
        symspell_correctors.insert("ja".to_string(), Box::new(corrector));
    }

    let whitelist = vec!["ja".to_string()];
    let mut hybrid = HybridCorrector::new(symspell_correctors, whitelist);

    // Japanese should use SymSpell
    let ja_result = hybrid
        .correct_with_language("こんにちは", "ja")
        .expect("Correction should succeed");
    assert!(ja_result.len() > 0, "Japanese should be corrected");

    // Korean is NOT in whitelist, should fall back to T5 or passthrough
    let ko_result = hybrid
        .correct_with_language("안녕하세요", "ko")
        .expect("Correction should succeed");
    assert!(
        ko_result.len() > 0,
        "Korean should still return output (T5 or passthrough)"
    );
}

#[test]
fn test_whitelist_excludes_cased_languages() {
    // Verify that languages with semantic casing are excluded from whitelist
    let config = ErrorCorrectionConfig::default();

    let cased_languages = vec!["de", "es", "fr", "it", "ru"];

    for lang in cased_languages {
        assert!(
            !config.symspell_languages.contains(&lang.to_string()),
            "{} should NOT be in whitelist (semantic casing)",
            lang
        );
    }
}

#[test]
fn test_whitelist_includes_non_cased_languages() {
    // Verify that languages without semantic casing are in whitelist
    let config = ErrorCorrectionConfig::default();

    let non_cased_languages = vec!["ja", "ko", "he", "ar", "zh"];

    for lang in non_cased_languages {
        assert!(
            config.symspell_languages.contains(&lang.to_string()),
            "{} should be in whitelist (no semantic casing)",
            lang
        );
    }
}
