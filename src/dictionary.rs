//! Catalog of available SymSpell frequency dictionaries.
//!
//! This module is always available (no feature flags) because it contains
//! only static metadata about available dictionaries. The actual download
//! functionality is in `models::download` and requires the `model-download` feature.

/// Metadata for a SymSpell frequency dictionary.
#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryInfo {
    /// Language code (e.g., "en", "de").
    pub language: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Dictionary filename.
    pub filename: &'static str,
    /// Raw GitHub URL for download.
    pub url: &'static str,
    /// Approximate download size in KB.
    pub size_kb: u32,
    /// SHA-256 checksum of the dictionary file for integrity verification.
    pub sha256: &'static str,
}

/// Available SymSpell frequency dictionaries, ordered by language code.
pub const DICTIONARIES: &[DictionaryInfo] = &[
    DictionaryInfo {
        language: "de",
        display_name: "German",
        filename: "de-100k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/de-100k.txt",
        size_kb: 1200,
        sha256: "a98c27cbe0921cb3a9927eb28639efb45fc3493f72466cca0622c64dff4e74a9",
    },
    DictionaryInfo {
        language: "en",
        display_name: "English",
        filename: "en-80k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/en-80k.txt",
        size_kb: 900,
        sha256: "f84bfae717ff3a4a3b90c824ed06ea08e4b2ed2746f7bba63d4e52f2c8bf85c3",
    },
    DictionaryInfo {
        language: "es",
        display_name: "Spanish",
        filename: "es-100l.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/es-100l.txt",
        size_kb: 1000,
        sha256: "fd538cb220cd00d0a9f20d2190ba6f033b76f91ee08c6c0df8fadbf46bbdf319",
    },
    DictionaryInfo {
        language: "fr",
        display_name: "French",
        filename: "fr-100k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/fr-100k.txt",
        size_kb: 1100,
        sha256: "b7dca46c0002daa0c6d70e1078bef4110815f65b0c90c35be1ae900ba2f5e9fc",
    },
    DictionaryInfo {
        language: "he",
        display_name: "Hebrew",
        filename: "he-100k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/he-100k.txt",
        size_kb: 800,
        sha256: "b1305dc929be951e20d50440585a627b4c9d95e2044122a73b33a0ba25b713d9",
    },
    DictionaryInfo {
        language: "it",
        display_name: "Italian",
        filename: "it-100k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/it-100k.txt",
        size_kb: 1100,
        sha256: "5f746afb7e6ae802872061ef025ce883cfa2a8779780968fa285dfd0907e9cfc",
    },
    DictionaryInfo {
        language: "ru",
        display_name: "Russian",
        filename: "ru-100k.txt",
        url: "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary/ru-100k.txt",
        size_kb: 1300,
        sha256: "2028262759546fdc346386369a59541d0464f48634c3103ff3f36d01518efcc6",
    },
];

/// Look up a dictionary by language code.
pub fn get_dictionary(lang: &str) -> Option<&'static DictionaryInfo> {
    DICTIONARIES.iter().find(|d| d.language == lang)
}

/// List all available dictionaries.
pub fn list_dictionaries() -> &'static [DictionaryInfo] {
    DICTIONARIES
}

/// Check if a dictionary exists for the given language.
pub fn has_dictionary(lang: &str) -> bool {
    DICTIONARIES.iter().any(|d| d.language == lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_dictionary_english() {
        let dict = get_dictionary("en").expect("en dictionary should exist");
        assert_eq!(dict.language, "en");
        assert_eq!(dict.display_name, "English");
        assert_eq!(dict.filename, "en-80k.txt");
        assert_eq!(dict.size_kb, 900);
        assert!(dict.url.contains("en-80k.txt"));
    }

    #[test]
    fn test_get_dictionary_german() {
        let dict = get_dictionary("de").expect("de dictionary should exist");
        assert_eq!(dict.language, "de");
        assert_eq!(dict.display_name, "German");
        assert_eq!(dict.filename, "de-100k.txt");
        assert_eq!(dict.size_kb, 1200);
    }

    #[test]
    fn test_get_dictionary_spanish() {
        let dict = get_dictionary("es").expect("es dictionary should exist");
        assert_eq!(dict.language, "es");
        assert_eq!(dict.display_name, "Spanish");
        assert_eq!(dict.filename, "es-100l.txt");
        assert_eq!(dict.size_kb, 1000);
    }

    #[test]
    fn test_get_dictionary_french() {
        let dict = get_dictionary("fr").expect("fr dictionary should exist");
        assert_eq!(dict.language, "fr");
        assert_eq!(dict.display_name, "French");
        assert_eq!(dict.filename, "fr-100k.txt");
        assert_eq!(dict.size_kb, 1100);
    }

    #[test]
    fn test_get_dictionary_hebrew() {
        let dict = get_dictionary("he").expect("he dictionary should exist");
        assert_eq!(dict.language, "he");
        assert_eq!(dict.display_name, "Hebrew");
        assert_eq!(dict.filename, "he-100k.txt");
        assert_eq!(dict.size_kb, 800);
    }

    #[test]
    fn test_get_dictionary_italian() {
        let dict = get_dictionary("it").expect("it dictionary should exist");
        assert_eq!(dict.language, "it");
        assert_eq!(dict.display_name, "Italian");
        assert_eq!(dict.filename, "it-100k.txt");
        assert_eq!(dict.size_kb, 1100);
    }

    #[test]
    fn test_get_dictionary_russian() {
        let dict = get_dictionary("ru").expect("ru dictionary should exist");
        assert_eq!(dict.language, "ru");
        assert_eq!(dict.display_name, "Russian");
        assert_eq!(dict.filename, "ru-100k.txt");
        assert_eq!(dict.size_kb, 1300);
    }

    #[test]
    fn test_get_dictionary_nonexistent() {
        assert!(get_dictionary("nonexistent").is_none());
        assert!(get_dictionary("").is_none());
        assert!(get_dictionary("xx").is_none());
    }

    #[test]
    fn test_list_dictionaries_count() {
        let dicts = list_dictionaries();
        assert_eq!(dicts.len(), 7);
    }

    #[test]
    fn test_list_dictionaries_ordered_by_language_code() {
        let dicts = list_dictionaries();
        for window in dicts.windows(2) {
            assert!(
                window[0].language < window[1].language,
                "{} should come before {}",
                window[0].language,
                window[1].language,
            );
        }
    }

    #[test]
    fn test_has_dictionary_true() {
        assert!(has_dictionary("en"));
        assert!(has_dictionary("de"));
        assert!(has_dictionary("es"));
        assert!(has_dictionary("fr"));
        assert!(has_dictionary("he"));
        assert!(has_dictionary("it"));
        assert!(has_dictionary("ru"));
    }

    #[test]
    fn test_has_dictionary_false() {
        assert!(!has_dictionary("nonexistent"));
        assert!(!has_dictionary(""));
        assert!(!has_dictionary("xx"));
        assert!(!has_dictionary("zh"));
    }

    #[test]
    fn test_all_urls_contain_base_url() {
        const BASE_URL: &str = "https://raw.githubusercontent.com/wolfgarbe/SymSpell/master/SymSpell.FrequencyDictionary";
        for dict in DICTIONARIES {
            assert!(
                dict.url.starts_with(BASE_URL),
                "{} URL should start with base URL",
                dict.language
            );
        }
    }

    #[test]
    fn test_all_urls_contain_filename() {
        for dict in DICTIONARIES {
            assert!(
                dict.url.contains(dict.filename),
                "{} URL should contain filename {}",
                dict.language,
                dict.filename
            );
        }
    }

    #[test]
    fn test_all_filenames_have_txt_extension() {
        for dict in DICTIONARIES {
            assert!(
                dict.filename.ends_with(".txt"),
                "{} filename should end with .txt: {}",
                dict.language,
                dict.filename
            );
        }
    }

    #[test]
    fn test_dictionary_info_clone() {
        let dict = get_dictionary("en").expect("should exist");
        let cloned = dict.clone();
        assert_eq!(dict, &cloned);
    }

    #[test]
    fn test_all_languages_unique() {
        let mut seen = std::collections::HashSet::new();
        for dict in DICTIONARIES {
            assert!(
                seen.insert(dict.language),
                "Duplicate language code: {}",
                dict.language
            );
        }
    }

    #[test]
    fn test_all_filenames_unique() {
        let mut seen = std::collections::HashSet::new();
        for dict in DICTIONARIES {
            assert!(
                seen.insert(dict.filename),
                "Duplicate filename: {}",
                dict.filename
            );
        }
    }

    #[test]
    fn test_all_sha256_hashes_non_empty() {
        for dict in DICTIONARIES {
            assert!(
                !dict.sha256.is_empty(),
                "{} dictionary should have a non-empty SHA-256 hash",
                dict.language
            );
            assert_eq!(
                dict.sha256.len(),
                64,
                "{} SHA-256 hash should be 64 hex characters, got {}",
                dict.language,
                dict.sha256.len()
            );
        }
    }

    #[test]
    fn test_all_sha256_hashes_are_valid_hex() {
        for dict in DICTIONARIES {
            let all_hex = dict.sha256.chars().all(|c| c.is_ascii_hexdigit());
            assert!(
                all_hex,
                "{} SHA-256 hash contains non-hex characters: {}",
                dict.language, dict.sha256
            );
        }
    }
}
