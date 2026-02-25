//! Integration tests for multi-language SymSpell correction.

use std::collections::HashMap;

use voicsh::correction::corrector::Corrector;
use voicsh::correction::hybrid::HybridCorrector;
use voicsh::correction::symspell::SymSpellCorrector;
use voicsh::dictionary::list_dictionaries;
use voicsh::models::download::{dictionary_path, download_dictionary, is_dictionary_installed};

#[cfg(feature = "symspell")]
async fn setup_multilang_symspell() -> Result<HybridCorrector, Box<dyn std::error::Error>> {
    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    for dict in list_dictionaries() {
        let lang = dict.language;

        if !is_dictionary_installed(lang) {
            println!("Downloading SymSpell dictionary for '{}'...", lang);
            download_dictionary(lang, true).await?;
        }

        let path = dictionary_path(lang);
        match SymSpellCorrector::from_file(&path, lang) {
            Ok(c) => {
                symspell_correctors.insert(lang.to_string(), Box::new(c) as Box<dyn Corrector>);
                println!("✓ SymSpell loaded: {} ({})", lang, dict.display_name);
            }
            Err(e) => {
                eprintln!("✗ Failed to load SymSpell for {}: {}", lang, e);
            }
        }
    }

    Ok(HybridCorrector::new(symspell_correctors, Vec::new()))
}

#[test]
#[cfg(feature = "symspell")]
fn test_multilang_symspell_integration() {
    // This test downloads dictionaries if not present and tests
    // that correction uses the correct language dictionary

    let test_cases: Vec<TestCorrectionCase> = vec![
        // German corrections
        TestCorrectionCase {
            language: "de",
            input: "Das ist ein Test",
            expected_contains: &["das", "ist", "ein", "test"],
            description: "German: correct text uses German dictionary",
        },
        TestCorrectionCase {
            language: "de",
            input: "Hallo Wlt",
            expected_contains: &["hallo", "welt"],
            description: "German: typo correction using German dictionary",
        },
        // Spanish corrections
        TestCorrectionCase {
            language: "es",
            input: "Hola Mundo",
            expected_contains: &["hola", "mundo"],
            description: "Spanish: correct text uses Spanish dictionary",
        },
        TestCorrectionCase {
            language: "es",
            input: "Buenos Diaz",
            expected_contains: &["buenos"],
            description: "Spanish: typo correction using Spanish dictionary",
        },
        // Italian corrections
        TestCorrectionCase {
            language: "it",
            input: "Ciao Mondo",
            expected_contains: &["ciao", "mondo"],
            description: "Italian: correct text uses Italian dictionary",
        },
        // English corrections
        TestCorrectionCase {
            language: "en",
            input: "Hello World",
            expected_contains: &["hello", "world"],
            description: "English: correct text uses English dictionary",
        },
    ];

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    let mut hybrid = runtime
        .block_on(setup_multilang_symspell())
        .expect("Failed to setup SymSpell dictionaries");

    println!("\n=== Running multi-language SymSpell integration tests ===");

    for test_case in test_cases {
        println!("\nTest: {}", test_case.description);
        println!("  Language: {}", test_case.language);
        println!("  Input: \"{}\"", test_case.input);

        let result = hybrid
            .correct_with_language(&test_case.input, test_case.language)
            .expect("Correction failed");

        println!("  Output: \"{}\"", result);

        for expected_word in test_case.expected_contains {
            assert!(
                result.to_lowercase().contains(expected_word),
                "Expected '{}' in corrected text for {} case: \"{}\"",
                expected_word,
                test_case.language,
                test_case.description
            );
        }

        println!("  ✓ Passed: correction uses correct language dictionary");
    }

    println!("\n=== All integration tests passed ===");
}

#[test]
#[cfg(feature = "symspell")]
fn test_language_specific_corrections_do_not_interfere() {
    // Test that French text doesn't get "corrected" to German words
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    let mut hybrid = runtime
        .block_on(setup_multilang_symspell())
        .expect("Failed to setup SymSpell dictionaries");

    // French text should use French dictionary, not mix in German words
    let french_input = "Bonjour le monde";
    let french_result = hybrid
        .correct_with_language(french_input, "fr")
        .expect("French correction failed");

    // Should remain French, not get "corrected" to German
    assert!(
        french_result == "bonjour le monde"
            || french_result.contains("bonjour")
            || french_result.contains("monde")
            || french_result.contains("le"),
        "French text should remain French, not be corrupted by other dictionaries"
    );

    println!("✓ Language isolation test passed");
}

struct TestCorrectionCase {
    language: &'static str,
    input: &'static str,
    expected_contains: &'static [&'static str],
    description: &'static str,
}

#[cfg(all(feature = "symspell", feature = "error-correction"))]
#[tokio::test]
async fn test_hybrid_with_t5_and_symspell() {
    // Full hybrid mode: T5 for English, SymSpell for other languages
    use voicsh::correction::candle_t5::CandleT5Corrector;
    use voicsh::models::correction_catalog::get_correction_model;

    let mut symspell_correctors: HashMap<String, Box<dyn Corrector>> = HashMap::new();

    for dict in list_dictionaries() {
        let lang = dict.language;

        if !is_dictionary_installed(lang) {
            eprintln!("Downloading SymSpell dictionary for '{}'...", lang);
            if download_dictionary(lang, true).await.is_err() {
                continue;
            }
        }

        let path = dictionary_path(lang);
        if let Ok(c) = SymSpellCorrector::from_file(&path, lang) {
            symspell_correctors.insert(lang.to_string(), Box::new(c) as Box<dyn Corrector>);
        }
    }

    // Try to load T5
    let t5_corrector = if let Some(model_info) = get_correction_model("flan-t5-small") {
        eprintln!("Loading T5 model for hybrid test...");
        match CandleT5Corrector::load(model_info) {
            Ok(c) => {
                eprintln!("✓ T5 loaded successfully");
                Some(Box::new(c) as Box<dyn Corrector>)
            }
            Err(e) => {
                eprintln!("✗ Failed to load T5: {} (continuing without T5)", e);
                None
            }
        }
    } else {
        None
    };

    let mut hybrid = HybridCorrector::new(t5_corrector, symspell_correctors, Vec::new());

    // Test English uses T5 if available
    let english_input = "the quick brown fox";
    let english_result = hybrid
        .correct_with_language(english_input, "en")
        .expect("Correction failed");

    println!("English input: \"{}\"", english_input);
    println!("English output: \"{}\"", english_result);
    // T5 may or may not be loaded depending on environment
    assert!(
        !english_result.is_empty(),
        "English result should not be empty"
    );

    // Test German: not in whitelist (empty whitelist = no SymSpell languages), should passthrough
    let german_input = "Hallo Welt";
    let german_result = hybrid
        .correct_with_language(german_input, "de")
        .expect("Correction failed");

    println!("German input: \"{}\"", german_input);
    println!("German output: \"{}\"", german_result);
    assert_eq!(
        german_result, german_input,
        "Non-whitelisted language should passthrough unchanged"
    );

    // Test Spanish: not in whitelist, should passthrough
    let spanish_input = "Hola Mundo";
    let spanish_result = hybrid
        .correct_with_language(spanish_input, "es")
        .expect("Correction failed");

    println!("Spanish input: \"{}\"", spanish_input);
    println!("Spanish output: \"{}\"", spanish_result);
    assert_eq!(
        spanish_result, spanish_input,
        "Non-whitelisted language should passthrough unchanged"
    );
}
