use crate::defaults;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
#[cfg(feature = "cli")]
use std::path::PathBuf;

/// Trailing punctuation characters stripped during filter normalization.
pub const FILTER_PUNCTUATION: [char; 9] = ['.', '!', '?', ',', ';', '。', '、', '！', '？'];

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    pub stt: SttConfig,
    #[serde(alias = "input")]
    pub injection: InjectionConfig,
    pub voice_commands: VoiceCommandConfig,
    pub transcription: TranscriptionConfig,
}

/// Audio capture configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioConfig {
    pub device: Option<String>,
    pub sample_rate: u32,
    pub vad_threshold: f32,
    pub silence_duration_ms: u32,
}

/// Speech-to-text configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SttConfig {
    pub model: String,
    pub language: String,
    pub fan_out: bool,
    /// Language allowlist for auto-detect mode. Empty = accept all.
    /// When non-empty, only transcriptions in these languages are accepted.
    pub allowed_languages: Vec<String>,
    /// Minimum confidence threshold. Transcriptions below this are dropped.
    /// 0.0 = accept all (default).
    pub min_confidence: f32,
}

/// Injection configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InjectionConfig {
    pub method: InjectionMethod,
    pub paste_key: String,
    pub backend: InjectionBackend,
}

/// Voice command configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VoiceCommandConfig {
    /// Enable voice command processing (default: true)
    pub enabled: bool,
    /// Disable all built-in voice commands (default: false)
    pub disable_defaults: bool,
    /// User-defined command overrides: spoken phrase → replacement text
    pub commands: std::collections::HashMap<String, String>,
}

impl Default for VoiceCommandConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            disable_defaults: false,
            commands: std::collections::HashMap::new(),
        }
    }
}

/// Error correction backend selection
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CorrectionBackend {
    /// Dictionary-based correction using SymSpell (fast, ~20 MB memory)
    Symspell,
    /// Neural correction using Flan-T5 (English only, requires model download)
    T5,
    /// Hybrid: T5 for English, SymSpell for other languages
    #[default]
    Hybrid,
}

/// Error correction configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ErrorCorrectionConfig {
    /// Enable post-ASR error correction.
    pub enabled: bool,
    /// Backend to use for correction
    pub backend: CorrectionBackend,
    /// T5 model size: "flan-t5-small" (64 MB), "flan-t5-base" (263 MB), "flan-t5-large" (852 MB).
    pub model: String,
    /// Only correct tokens with probability below this threshold (0.0-1.0).
    pub confidence_threshold: f32,
    /// Dictionary language for SymSpell backend.
    /// "auto" = match STT language if dictionary exists, fall back to "en".
    pub dictionary_language: String,
    /// Languages enabled for SymSpell correction (only used with hybrid backend).
    /// SymSpell lowercases all output, so only enable for languages where
    /// lowercase is acceptable: Hebrew (he), Arabic (ar), Chinese (zh), Japanese (ja), Korean (ko).
    /// Empty or "auto" = enable all available languages.
    pub symspell_languages: Vec<String>,
}

impl Default for ErrorCorrectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: CorrectionBackend::Hybrid,
            model: "flan-t5-base".to_string(),
            confidence_threshold: 0.65,
            dictionary_language: "auto".to_string(),
            symspell_languages: vec![
                "he".to_string(),
                "ar".to_string(),
                "zh".to_string(),
                "ja".to_string(),
                "ko".to_string(),
            ],
        }
    }
}

/// Transcription post-processing configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub hallucination_filters: HallucinationFilterConfig,
    pub error_correction: ErrorCorrectionConfig,
}

/// Hallucination filter configuration.
///
/// All built-in defaults from all languages are always active (Whisper sometimes
/// returns text in the wrong language, so filtering by configured language is unreliable).
///
/// Users can add extra phrases or override specific language defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HallucinationFilterConfig {
    /// Extra phrases to filter, merged on top of built-in defaults.
    pub add: Vec<String>,
    /// Extra suspect phrases to soft-filter, merged on top of built-in defaults.
    pub suspect_add: Vec<String>,
    /// Per-language overrides. If a language key is present, it REPLACES
    /// that language's built-in defaults. Empty vec = disable that language.
    /// Languages not listed here keep their built-in defaults.
    #[serde(flatten)]
    pub overrides: HashMap<String, Vec<String>>,
}

/// Injection method enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InjectionMethod {
    Clipboard,
    Direct,
}

/// Injection backend selection
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InjectionBackend {
    #[default]
    Auto,
    Portal,
    Wtype,
    Ydotool,
}

impl std::str::FromStr for InjectionBackend {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "portal" => Ok(Self::Portal),
            "wtype" => Ok(Self::Wtype),
            "ydotool" => Ok(Self::Ydotool),
            other => Err(format!(
                "Unknown backend '{}'. Valid options: auto, portal, wtype, ydotool",
                other
            )),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device: None,
            sample_rate: defaults::SAMPLE_RATE,
            vad_threshold: defaults::VAD_THRESHOLD,
            silence_duration_ms: defaults::SILENCE_DURATION_MS,
        }
    }
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model: defaults::DEFAULT_MODEL.to_string(),
            language: defaults::DEFAULT_LANGUAGE.to_string(),
            fan_out: false,
            allowed_languages: Vec::new(),
            min_confidence: 0.0,
        }
    }
}

impl Default for InjectionConfig {
    fn default() -> Self {
        Self {
            method: InjectionMethod::Clipboard,
            paste_key: "auto".to_string(),
            backend: InjectionBackend::Auto,
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    ///
    /// Returns an error if the file contains invalid TOML.
    /// Missing fields will use default values.
    pub fn load(path: &Path) -> crate::error::Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::error::VoicshError::ConfigFileNotFound {
                    path: path.display().to_string(),
                }
            } else {
                crate::error::VoicshError::Io(e)
            }
        })?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Load configuration from a file or return defaults if file doesn't exist
    ///
    /// Only returns defaults if the file is missing.
    /// Returns errors for invalid TOML.
    pub fn load_or_default(path: &Path) -> Self {
        match Self::load(path) {
            Ok(config) => config,
            Err(crate::error::VoicshError::ConfigFileNotFound { .. }) => Self::default(),
            Err(e) => {
                // Re-panic on invalid TOML or other errors
                panic!("Failed to load config from {}: {}", path.display(), e);
            }
        }
    }

    /// Apply environment variable overrides
    ///
    /// Supported environment variables:
    /// - VOICSH_MODEL → stt.model
    /// - VOICSH_LANGUAGE → stt.language
    /// - VOICSH_AUDIO_DEVICE → audio.device
    /// - VOICSH_BACKEND → injection.backend
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(model) = std::env::var("VOICSH_MODEL")
            && !model.is_empty()
        {
            self.stt.model = model;
        }

        if let Ok(language) = std::env::var("VOICSH_LANGUAGE")
            && !language.is_empty()
        {
            self.stt.language = language;
        }

        if let Ok(device) = std::env::var("VOICSH_AUDIO_DEVICE")
            && !device.is_empty()
        {
            self.audio.device = Some(device);
        }

        if let Ok(backend) = std::env::var("VOICSH_BACKEND")
            && !backend.is_empty()
        {
            self.injection.backend = backend.parse().unwrap_or_else(|msg| {
                eprintln!("Warning: {msg}, using Auto");
                InjectionBackend::Auto
            });
        }

        self
    }

    /// Get the default configuration file path
    ///
    /// Returns ~/.config/voicsh/config.toml on Linux
    #[cfg(feature = "cli")]
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| {
                eprintln!("voicsh: could not determine config directory, using ~/.config");
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
                    .join(".config")
            })
            .join("voicsh")
            .join("config.toml")
    }

    /// Serialize this configuration to TOML and write it to `path`.
    ///
    /// Creates parent directories if they do not exist.
    pub fn save(&self, path: &Path) -> crate::error::Result<()> {
        let toml_str = toml::to_string_pretty(self).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config to TOML: {e}"))
        })?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                crate::error::VoicshError::Other(format!(
                    "Failed to create config directory '{}': {e}",
                    parent.display()
                ))
            })?;
        }

        fs::write(path, toml_str).map_err(|e| {
            crate::error::VoicshError::Other(format!(
                "Failed to write config to '{}': {e}",
                path.display()
            ))
        })?;

        Ok(())
    }

    /// Load existing config from `path` (or defaults if missing), update
    /// `stt.model` to `model`, and save back.
    ///
    /// This is used by `voicsh init` to update just the model without
    /// clobbering other user settings.
    pub fn update_model(path: &Path, model: &str) -> crate::error::Result<()> {
        let mut config = match Self::load(path) {
            Ok(cfg) => cfg,
            Err(crate::error::VoicshError::ConfigFileNotFound { .. }) => Self::default(),
            Err(e) => return Err(e),
        };
        config.stt.model = model.to_string();
        config.save(path)
    }

    /// Get a config value by dotted path (e.g. "stt.model").
    pub fn get_value_by_path(&self, key: &str) -> crate::error::Result<String> {
        let value = toml::Value::try_from(self).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config: {e}"))
        })?;
        let leaf = navigate_toml_path(&value, key)?;
        Ok(format_toml_value(leaf))
    }

    /// Set a config value by dotted path and save.
    ///
    /// Loads the existing config (or defaults), sets the value, validates
    /// by deserializing back, then saves.
    pub fn set_value_by_path(path: &Path, key: &str, value_str: &str) -> crate::error::Result<()> {
        let config = match Self::load(path) {
            Ok(cfg) => cfg,
            Err(crate::error::VoicshError::ConfigFileNotFound { .. }) => Self::default(),
            Err(e) => return Err(e),
        };

        let mut root = toml::Value::try_from(&config).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config: {e}"))
        })?;

        let new_value = parse_toml_value(value_str);
        set_toml_path(&mut root, key, new_value)?;

        // Validate by deserializing back
        let toml_str = toml::to_string_pretty(&root).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config: {e}"))
        })?;
        let _validated: Config = toml::from_str(&toml_str)?;

        // Save the validated config
        _validated.save(path)
    }

    /// Serialize this configuration to pretty TOML for display.
    pub fn to_display_toml(&self) -> crate::error::Result<String> {
        toml::to_string_pretty(self).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config to TOML: {e}"))
        })
    }

    /// Display a single config section by top-level key (e.g., "stt", "audio").
    pub fn display_section(&self, key: &str) -> crate::error::Result<String> {
        let value = toml::Value::try_from(self).map_err(|e| {
            crate::error::VoicshError::Other(format!("Failed to serialize config: {e}"))
        })?;
        let section = navigate_toml_path(&value, key)?;
        Ok(format_toml_value(section))
    }

    /// Format voice commands for display, filtered by language(s).
    ///
    /// Shows built-in commands for each requested language, plus any
    /// custom commands from the config.
    pub fn display_voice_commands(languages: &[&str], custom: &HashMap<String, String>) -> String {
        use crate::pipeline::post_processor::builtin_commands_display;

        let mut out = String::new();
        for lang in languages {
            let lang_name = language_name(lang);
            let builtins = builtin_commands_display(lang);
            if builtins.is_empty() {
                continue;
            }
            out.push_str(&format!("Voice commands ({}, {}):\n", lang_name, lang));
            for (phrase, replacement) in &builtins {
                let display_replacement = replacement.replace('\n', "\\n").replace('\t', "\\t");
                let quoted_phrase = format!("\"{}\"", phrase);
                out.push_str(&format!(
                    "  {:<30} → \"{}\"\n",
                    quoted_phrase, display_replacement
                ));
            }
            out.push('\n');
        }

        if !custom.is_empty() {
            out.push_str("Custom commands:\n");
            let mut sorted: Vec<_> = custom.iter().collect();
            sorted.sort_by_key(|(k, _)| k.to_lowercase());
            for (phrase, replacement) in sorted {
                out.push_str(&format!(
                    "  {:30} → \"{}\"\n",
                    format!("\"{}\"", phrase),
                    replacement
                ));
            }
            out.push('\n');
        }

        out
    }

    /// Validate language codes against supported languages.
    ///
    /// Returns an error naming the unsupported language if any are invalid.
    pub fn validate_languages(languages: &[&str]) -> crate::error::Result<()> {
        use crate::pipeline::post_processor::SUPPORTED_LANGUAGES;
        for lang in languages {
            if !SUPPORTED_LANGUAGES.contains(lang) {
                return Err(crate::error::VoicshError::Other(format!(
                    "Unknown language '{}'. Supported: {}",
                    lang,
                    SUPPORTED_LANGUAGES.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// Generate a commented TOML template documenting all fields and defaults.
    pub fn dump_template() -> String {
        let defaults = default_hallucination_filters();
        let mut lang_keys: Vec<&String> = defaults.keys().collect();
        lang_keys.sort();

        let mut out = String::new();
        out.push_str("# voicsh configuration\n");
        out.push_str("# Save to: ~/.config/voicsh/config.toml\n");
        out.push('\n');

        out.push_str("[audio]\n");
        out.push_str("# device = \"hw:0,0\"  # Audio input device (default: system default)\n");
        out.push_str(&format!(
            "# sample_rate = {}  # Sample rate in Hz\n",
            defaults::SAMPLE_RATE
        ));
        out.push_str(&format!(
            "# vad_threshold = {}  # Voice activity detection threshold (0.0-1.0)\n",
            defaults::VAD_THRESHOLD
        ));
        out.push_str(&format!(
            "# silence_duration_ms = {}  # Silence before speech end (ms)\n",
            defaults::SILENCE_DURATION_MS
        ));
        out.push('\n');

        out.push_str("[stt]\n");
        out.push_str(&format!(
            "# model = \"{}\"  # Whisper model name\n",
            defaults::DEFAULT_MODEL
        ));
        out.push_str(&format!(
            "# language = \"{}\"  # Language code (auto, en, de, es, fr, ...)\n",
            defaults::DEFAULT_LANGUAGE
        ));
        out.push_str("# fan_out = false  # Run multilingual + English models in parallel\n");
        out.push_str(
            "# allowed_languages = [\"en\", \"de\"]  # Only accept these languages (empty = all)\n",
        );
        out.push_str(
            "# min_confidence = 0.0  # Minimum confidence threshold (0.0-1.0, 0 = accept all)\n",
        );
        out.push('\n');

        out.push_str("[injection]\n");
        out.push_str("# method = \"Clipboard\"  # Injection method: Clipboard or Direct\n");
        out.push_str("# paste_key = \"auto\"  # Paste key combo (auto, ctrl+v, ctrl+shift+v)\n");
        out.push_str("# backend = \"auto\"  # Injection backend: auto, portal, wtype, ydotool\n");
        out.push('\n');

        out.push_str("[voice_commands]\n");
        out.push_str("# enabled = true  # Enable voice command processing\n");
        out.push_str(
            "# disable_defaults = false  # Set to true to disable all built-in commands\n",
        );
        out.push_str("# [voice_commands.commands]\n");
        out.push_str("# \"smiley\" = \":)\"  # Custom voice command mappings\n");
        out.push_str("#\n");
        out.push_str("# Built-in commands (active unless disable_defaults = true):\n");

        // Show subset of languages with their built-in commands
        let sample_langs = ["en", "de", "es", "fr"];
        for lang in &sample_langs {
            let builtins = crate::pipeline::post_processor::builtin_commands_display(lang);
            if builtins.is_empty() {
                continue;
            }

            // Show first 4 commands as examples
            let examples: Vec<String> = builtins
                .iter()
                .take(4)
                .map(|(phrase, replacement)| format!("\"{}\" → \"{}\"", phrase, replacement))
                .collect();

            let lang_name = match *lang {
                "en" => "English",
                "de" => "German",
                "es" => "Spanish",
                "fr" => "French",
                _ => lang,
            };

            out.push_str(&format!(
                "# {} ({}): {}, ...\n",
                lang_name,
                lang,
                examples.join(", ")
            ));
        }
        out.push('\n');

        out.push_str("[transcription.error_correction]\n");
        out.push_str("# enabled = true  # Post-ASR error correction\n");
        out.push_str("# backend = \"hybrid\"  # Backend: t5 (English, neural), symspell (multi-language, dictionary), hybrid (t5 for en, symspell for others)\n");
        out.push_str("# model = \"flan-t5-base\"  # T5 model (only used when backend = \"t5\" or \"hybrid\")\n");
        out.push_str(
            "# confidence_threshold = 0.65  # Only correct tokens below this probability (0.0-1.0)\n",
        );
        out.push_str("# dictionary_language = \"auto\"  # SymSpell dictionary language: auto, en, de, es, fr, he, it, ru\n");
        out.push_str("# symspell_languages = [\"he\", \"ar\", \"zh\", \"ja\", \"ko\"]  # Languages for SymSpell in hybrid mode (SymSpell lowercases output, so use with languages where lowercase is acceptable)\n");
        out.push('\n');

        out.push_str("[transcription.hallucination_filters]\n");
        out.push_str("# Extra phrases to filter (added on top of all built-in defaults):\n");
        out.push_str("# add = [\"my custom phrase\", \"another artifact\"]\n");
        out.push_str("#\n");
        out.push_str(
            "# Override a specific language's built-in defaults (replaces, not merges):\n",
        );
        out.push_str("# en = [\"Only these for English\"]  # replaces English defaults\n");
        out.push_str("# ko = []  # disables Korean defaults entirely\n");
        out.push_str("#\n");
        out.push_str("# Built-in defaults per language (always active unless overridden):\n");

        for lang in &lang_keys {
            let phrases = &defaults[lang.as_str()];
            let quoted: Vec<String> = phrases.iter().map(|p| format!("\"{}\"", p)).collect();
            out.push_str(&format!("# {} = [{}]\n", lang, quoted.join(", ")));
        }

        out
    }
}

/// Map a language code to a human-readable name.
fn language_name(code: &str) -> &'static str {
    match code {
        "en" => "English",
        "de" => "German",
        "es" => "Spanish",
        "fr" => "French",
        "pt" => "Portuguese",
        "it" => "Italian",
        "nl" => "Dutch",
        "pl" => "Polish",
        "ru" => "Russian",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "ko" => "Korean",
        "ar" => "Arabic",
        "tr" => "Turkish",
        _ => "Unknown",
    }
}

/// Parsed hallucination filter data from the embedded TOML.
struct ParsedFilterData {
    phrases: HashMap<String, Vec<String>>,
    suspect_phrases: HashMap<String, Vec<String>>,
}

/// Parse the embedded `hallucination_filters.toml` once, returning both
/// hard-filter phrases and suspect phrases keyed by language.
fn parse_filter_toml() -> ParsedFilterData {
    static TOML_DATA: &str = include_str!("hallucination_filters.toml");

    #[derive(serde::Deserialize)]
    struct LangEntry {
        phrases: Vec<String>,
        #[serde(default)]
        suspect_phrases: Vec<String>,
    }

    let parsed: HashMap<String, LangEntry> = toml::from_str(TOML_DATA)
        .unwrap_or_else(|e| panic!("embedded hallucination_filters.toml is invalid: {e}"));

    let mut phrases = HashMap::new();
    let mut suspect = HashMap::new();
    for (k, v) in parsed {
        phrases.insert(k.clone(), v.phrases);
        suspect.insert(k, v.suspect_phrases);
    }

    ParsedFilterData {
        phrases,
        suspect_phrases: suspect,
    }
}

/// Built-in hallucination filter defaults keyed by language.
///
/// Phrases are loaded from `hallucination_filters.toml` at compile time.
pub fn default_hallucination_filters() -> HashMap<String, Vec<String>> {
    parse_filter_toml().phrases
}

/// Built-in suspect phrase defaults keyed by language.
///
/// These are short filler words that could be real speech but are frequent
/// Whisper hallucinations on silence/noise. Loaded from `hallucination_filters.toml`
/// at compile time.
pub fn default_suspect_phrases() -> HashMap<String, Vec<String>> {
    parse_filter_toml().suspect_phrases
}

/// Resolve the active hallucination filter set from config.
///
/// 1. Start with built-in defaults per language
/// 2. For each language key in overrides, replace that language's defaults
/// 3. Flatten all languages into a single set
/// 4. Merge all entries from `add`
/// 5. Return combined set (lowercased; both original and punctuation-stripped
///    variants are stored for O(1) lookup without per-entry stripping at runtime)
pub fn resolve_hallucination_filters(config: &HallucinationFilterConfig) -> HashSet<String> {
    let mut defaults = default_hallucination_filters();

    // Apply per-language overrides
    for (lang, phrases) in &config.overrides {
        defaults.insert(lang.clone(), phrases.clone());
    }

    let mut result = HashSet::new();
    for phrase in defaults.into_values().flat_map(|p| p.into_iter()) {
        insert_with_punctuation_variant(&mut result, phrase.to_lowercase());
    }
    for phrase in &config.add {
        insert_with_punctuation_variant(&mut result, phrase.to_lowercase());
    }
    result
}

/// Resolve the active suspect phrase set from config.
///
/// 1. Start with built-in suspect phrases per language
/// 2. Flatten all languages into a single set
/// 3. Merge all entries from `suspect_add`
/// 4. Return combined set (lowercased; both original and punctuation-stripped
///    variants are stored for O(1) lookup without per-entry stripping at runtime)
pub fn resolve_suspect_phrases(config: &HallucinationFilterConfig) -> HashSet<String> {
    let defaults = default_suspect_phrases();

    let mut result = HashSet::new();
    for phrase in defaults.into_values().flat_map(|p| p.into_iter()) {
        insert_with_punctuation_variant(&mut result, phrase.to_lowercase());
    }
    for phrase in &config.suspect_add {
        insert_with_punctuation_variant(&mut result, phrase.to_lowercase());
    }
    result
}

/// Insert a lowercased phrase and its punctuation-stripped variant into the set.
///
/// Storing both forms means runtime matching only needs `HashSet::contains`,
/// with no per-entry iteration or stripping.
pub(crate) fn insert_with_punctuation_variant(set: &mut HashSet<String>, phrase: String) {
    let stripped = phrase.trim_end_matches(FILTER_PUNCTUATION).to_string();
    if stripped != phrase {
        set.insert(stripped);
    }
    set.insert(phrase);
}

/// Navigate a TOML value by dotted path.
fn navigate_toml_path<'a>(
    value: &'a toml::Value,
    path: &str,
) -> crate::error::Result<&'a toml::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in &parts {
        current =
            current
                .get(part)
                .ok_or_else(|| crate::error::VoicshError::ConfigInvalidValue {
                    key: path.to_string(),
                    message: format!("key '{}' not found", part),
                })?;
    }
    Ok(current)
}

/// Set a value in a TOML tree at a dotted path, creating intermediate tables as needed.
fn set_toml_path(
    root: &mut toml::Value,
    path: &str,
    value: toml::Value,
) -> crate::error::Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Set the leaf value
            if let toml::Value::Table(table) = current {
                table.insert(part.to_string(), value);
                return Ok(());
            }
            return Err(crate::error::VoicshError::ConfigInvalidValue {
                key: path.to_string(),
                message: format!("'{}' is not a table", parts[..i].join(".")),
            });
        }
        // Navigate or create intermediate table
        if let toml::Value::Table(table) = current {
            current = table
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        } else {
            return Err(crate::error::VoicshError::ConfigInvalidValue {
                key: path.to_string(),
                message: format!("'{}' is not a table", parts[..i].join(".")),
            });
        }
    }
    Ok(())
}

/// Parse a string into a TOML value, trying integer, float, bool, then string.
fn parse_toml_value(s: &str) -> toml::Value {
    if let Ok(i) = s.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return toml::Value::Float(f);
    }
    match s {
        "true" => return toml::Value::Boolean(true),
        "false" => return toml::Value::Boolean(false),
        _ => {}
    }
    toml::Value::String(s.to_string())
}

/// Format a TOML value for display. Strings are unwrapped (no quotes),
/// tables and arrays are pretty-printed.
fn format_toml_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Table(_) | toml::Value::Array(_) => {
            toml::to_string_pretty(value).unwrap_or_else(|_| format!("{value:?}"))
        }
        toml::Value::Datetime(dt) => dt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    // Mutex to serialize tests that modify environment variables
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_env(key: &str, value: &str) {
        crate::sys::set_env(key, value);
    }

    fn remove_env(key: &str) {
        crate::sys::remove_env(key);
    }

    fn clear_voicsh_env() {
        remove_env("VOICSH_MODEL");
        remove_env("VOICSH_LANGUAGE");
        remove_env("VOICSH_AUDIO_DEVICE");
        remove_env("VOICSH_BACKEND");
    }

    #[test]
    fn test_default_config_has_correct_values() {
        let config = Config::default();

        // Audio defaults
        assert_eq!(config.audio.device, None);
        assert_eq!(config.audio.sample_rate, 16000);
        assert_eq!(config.audio.vad_threshold, 0.02);
        assert_eq!(config.audio.silence_duration_ms, 1500);

        // STT defaults
        assert_eq!(config.stt.model, "base");
        assert_eq!(config.stt.language, "auto");

        // Injection defaults
        assert_eq!(config.injection.method, InjectionMethod::Clipboard);
        assert_eq!(config.injection.paste_key, "auto");
    }

    #[test]
    fn test_load_from_toml_file() {
        let toml_content = r#"
            [audio]
            device = "hw:0,0"
            sample_rate = 48000
            vad_threshold = 0.05
            silence_duration_ms = 2000

            [stt]
            model = "large-v3"
            language = "es"

            [injection]
            method = "Direct"
            paste_key = "ctrl+shift+v"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();

        assert_eq!(config.audio.device, Some("hw:0,0".to_string()));
        assert_eq!(config.audio.sample_rate, 48000);
        assert_eq!(config.audio.vad_threshold, 0.05);
        assert_eq!(config.audio.silence_duration_ms, 2000);

        assert_eq!(config.stt.model, "large-v3");
        assert_eq!(config.stt.language, "es");

        assert_eq!(config.injection.method, InjectionMethod::Direct);
        assert_eq!(config.injection.paste_key, "ctrl+shift+v");
    }

    #[test]
    fn test_load_from_toml_with_old_input_section() {
        // Test backward compatibility: old [input] section should map to injection field
        let toml_content = r#"
            [stt]
            model = "base"
            language = "en"

            [input]
            method = "Direct"
            paste_key = "ctrl+v"
            backend = "portal"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();

        assert_eq!(config.injection.method, InjectionMethod::Direct);
        assert_eq!(config.injection.paste_key, "ctrl+v");
        assert_eq!(config.injection.backend, InjectionBackend::Portal);
    }

    #[test]
    fn test_load_partial_config_uses_defaults() {
        let toml_content = r#"
            [stt]
            model = "small.en"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();

        // Only model should be overridden
        assert_eq!(config.stt.model, "small.en");

        // Everything else should be defaults
        assert_eq!(config.audio.device, None);
        assert_eq!(config.audio.sample_rate, 16000);
        assert_eq!(config.audio.vad_threshold, 0.02);
        assert_eq!(config.audio.silence_duration_ms, 1500);
        assert_eq!(config.stt.language, "auto");
        assert_eq!(config.injection.method, InjectionMethod::Clipboard);
        assert_eq!(config.injection.paste_key, "auto");
    }

    #[test]
    fn test_env_override_model() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_MODEL", "tiny.en");
        let config = Config::default().with_env_overrides();

        assert_eq!(config.stt.model, "tiny.en");
        assert_eq!(config.stt.language, "auto"); // Not overridden

        clear_voicsh_env();
    }

    #[test]
    fn test_env_override_device() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_AUDIO_DEVICE", "hw:1,0");
        let config = Config::default().with_env_overrides();

        assert_eq!(config.audio.device, Some("hw:1,0".to_string()));

        clear_voicsh_env();
    }

    #[test]
    fn test_env_override_all() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_MODEL", "medium.en");
        set_env("VOICSH_LANGUAGE", "fr");
        set_env("VOICSH_AUDIO_DEVICE", "pulse");

        let config = Config::default().with_env_overrides();

        assert_eq!(config.stt.model, "medium.en");
        assert_eq!(config.stt.language, "fr");
        assert_eq!(config.audio.device, Some("pulse".to_string()));

        clear_voicsh_env();
    }

    #[test]
    fn test_env_override_empty_string_ignored() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_MODEL", "");
        let config = Config::default().with_env_overrides();

        // Empty string should not override default
        assert_eq!(config.stt.model, "base");

        clear_voicsh_env();
    }

    #[test]
    fn test_env_override_backend() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_BACKEND", "portal");
        let config = Config::default().with_env_overrides();
        assert_eq!(config.injection.backend, InjectionBackend::Portal);

        set_env("VOICSH_BACKEND", "WTYPE");
        let config = Config::default().with_env_overrides();
        assert_eq!(config.injection.backend, InjectionBackend::Wtype);

        // Unknown backend falls back to Auto with warning
        set_env("VOICSH_BACKEND", "nonexistent");
        let config = Config::default().with_env_overrides();
        assert_eq!(config.injection.backend, InjectionBackend::Auto);

        clear_voicsh_env();
    }

    #[test]
    fn test_injection_backend_from_str() {
        assert_eq!(
            "auto".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Auto)
        );
        assert_eq!(
            "portal".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Portal)
        );
        assert_eq!(
            "wtype".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Wtype)
        );
        assert_eq!(
            "ydotool".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Ydotool)
        );
        // Case-insensitive
        assert_eq!(
            "PORTAL".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Portal)
        );
        assert_eq!(
            "Wtype".parse::<InjectionBackend>(),
            Ok(InjectionBackend::Wtype)
        );
        // Invalid
        assert!("invalid".parse::<InjectionBackend>().is_err());
        let err = "bogus".parse::<InjectionBackend>().unwrap_err();
        assert!(
            err.contains("bogus"),
            "Error should contain the invalid value: {err}"
        );
        assert!(
            err.contains("Valid options"),
            "Error should list valid options: {err}"
        );
    }

    #[test]
    fn test_invalid_toml_returns_error() {
        let invalid_toml = r#"
            [audio
            device = "broken
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_toml.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());

        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "cli")]
    fn test_default_path_is_xdg_compliant() {
        let path = Config::default_path();
        let path_str = path.to_string_lossy();

        // Should contain .config/voicsh/config.toml
        assert!(path_str.contains(".config"));
        assert!(path_str.contains("voicsh"));
        assert!(path_str.ends_with("config.toml"));
    }

    #[test]
    fn test_load_or_default_returns_default_for_missing_file() {
        let missing_path = Path::new("/tmp/nonexistent_voicsh_config_12345.toml");
        let config = Config::load_or_default(missing_path);

        // Should return defaults
        assert_eq!(config, Config::default());
    }

    #[test]
    #[should_panic(expected = "Failed to load config")]
    fn test_load_or_default_panics_on_invalid_toml() {
        let invalid_toml = r#"
            [audio
            device = "broken
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_toml.as_bytes()).unwrap();

        // Should panic on invalid TOML, not return defaults
        Config::load_or_default(temp_file.path());
    }

    // Malformed config tests
    #[test]
    fn test_malformed_config_invalid_toml_syntax() {
        let invalid_toml = r#"
            this is not valid TOML at all
            just some random text
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(invalid_toml.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        assert!(result.is_err(), "Should reject invalid TOML syntax");
    }

    #[test]
    fn test_malformed_config_wrong_data_types() {
        let wrong_types = r#"
            [audio]
            sample_rate = "not a number"
            vad_threshold = [1, 2, 3]
            silence_duration_ms = true
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(wrong_types.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        assert!(result.is_err(), "Should reject wrong data types");
    }

    #[test]
    fn test_malformed_config_negative_values() {
        let negative_values = r#"
            [audio]
            sample_rate = -16000
            vad_threshold = -0.5
            silence_duration_ms = -1000
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(negative_values.as_bytes()).unwrap();

        // TOML parsing might succeed but values would be nonsensical
        let result = Config::load(temp_file.path());
        if let Ok(config) = result {
            // If it loads, the values should have been converted to unsigned
            // or the parsing library might reject them
            assert!(
                config.audio.sample_rate > 0,
                "Sample rate should be positive"
            );
        }
    }

    #[test]
    fn test_malformed_config_unknown_sections() {
        let unknown_sections = r#"
            [unknown_section]
            unknown_field = "value"

            [another_bad_section]
            bad_field = 123
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(unknown_sections.as_bytes()).unwrap();

        // TOML with unknown sections might parse but be ignored
        let result = Config::load(temp_file.path());
        // Depending on serde settings, this might succeed or fail
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_malformed_config_empty_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"").unwrap();

        let result = Config::load(temp_file.path());
        // Empty TOML is valid and should return all defaults
        assert!(result.is_ok(), "Empty config should be valid");
        if let Ok(config) = result {
            assert_eq!(
                config,
                Config::default(),
                "Empty config should equal defaults"
            );
        }
    }

    #[test]
    fn test_malformed_config_only_whitespace() {
        let whitespace = "   \n\t\n   \n\t\t\n   ";

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(whitespace.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        assert!(result.is_ok(), "Whitespace-only config should be valid");
    }

    #[test]
    fn test_malformed_config_mixed_valid_invalid() {
        let mixed = r#"
            [audio]
            sample_rate = 44100

            this line is invalid TOML

            [stt]
            model = "base"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(mixed.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        assert!(result.is_err(), "Should reject mixed valid/invalid TOML");
    }

    #[test]
    fn test_malformed_config_duplicate_keys() {
        let duplicates = r#"
            [audio]
            device = "first"
            device = "second"
            device = "third"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(duplicates.as_bytes()).unwrap();

        // TOML behavior with duplicate keys varies by parser
        let result = Config::load(temp_file.path());
        if let Ok(config) = result {
            // Last value typically wins
            assert!(
                config.audio.device.is_some(),
                "Duplicate keys should resolve to some value"
            );
        }
    }

    #[test]
    fn test_malformed_config_extremely_large_values() {
        let huge_values = r#"
            [audio]
            sample_rate = 999999999999
            silence_duration_ms = 999999999999
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(huge_values.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        // Might succeed or fail depending on integer overflow handling
        let _ = result;
    }

    #[test]
    fn test_malformed_config_unicode_in_values() {
        let unicode_config = r#"
            [stt]
            model = "模型"
            language = "中文"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(unicode_config.as_bytes()).unwrap();

        let result = Config::load(temp_file.path());
        assert!(result.is_ok(), "Unicode in TOML values should be valid");
        if let Ok(config) = result {
            assert_eq!(
                config.stt.model, "模型",
                "Unicode model name should be preserved"
            );
            assert_eq!(
                config.stt.language, "中文",
                "Unicode language should be preserved"
            );
        }
    }

    // ── Voice commands config tests ──────────────────────────────────────

    #[test]
    fn test_default_voice_commands_enabled() {
        let config = Config::default();
        assert!(
            config.voice_commands.enabled,
            "Voice commands should be enabled by default"
        );
        assert!(
            config.voice_commands.commands.is_empty(),
            "No user overrides by default"
        );
    }

    #[test]
    fn test_voice_commands_from_toml() {
        let toml_content = r#"
            [voice_commands]
            enabled = true

            [voice_commands.commands]
            "smiley" = ":)"
            "at sign" = "@"
            "shrug" = '¯\_(ツ)_/¯'
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.voice_commands.enabled);
        assert_eq!(config.voice_commands.commands.len(), 3);
        assert_eq!(
            config.voice_commands.commands.get("smiley"),
            Some(&":)".to_string())
        );
        assert_eq!(
            config.voice_commands.commands.get("at sign"),
            Some(&"@".to_string())
        );
        assert_eq!(
            config.voice_commands.commands.get("shrug"),
            Some(&"¯\\_(ツ)_/¯".to_string())
        );
    }

    #[test]
    fn test_voice_commands_disabled() {
        let toml_content = r#"
            [voice_commands]
            enabled = false
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(
            !config.voice_commands.enabled,
            "Voice commands should be disabled"
        );
    }

    #[test]
    fn test_voice_commands_omitted_uses_defaults() {
        // Config with no [voice_commands] section at all
        let toml_content = r#"
            [stt]
            model = "small.en"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(
            config.voice_commands.enabled,
            "Voice commands should default to enabled when section omitted"
        );
        assert!(
            config.voice_commands.commands.is_empty(),
            "No user overrides when section omitted"
        );
    }

    #[test]
    fn test_voice_commands_empty_commands_table() {
        let toml_content = r#"
            [voice_commands]
            enabled = true

            [voice_commands.commands]
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.voice_commands.enabled);
        assert!(
            config.voice_commands.commands.is_empty(),
            "Empty commands table should produce empty map"
        );
    }

    // ── save / update_model tests ────────────────────────────────────────

    #[test]
    fn test_save_and_reload() {
        let config = Config {
            stt: SttConfig {
                model: "small".to_string(),
                language: "de".to_string(),
                fan_out: false,
                allowed_languages: Vec::new(),
                min_confidence: 0.0,
            },
            ..Config::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        config.save(&path).unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.stt.model, "small");
        assert_eq!(reloaded.stt.language, "de");
        assert_eq!(reloaded.audio.sample_rate, 16000); // default preserved
    }

    #[test]
    fn test_update_model_preserves_other_settings() {
        let config = Config {
            stt: SttConfig {
                model: "tiny".to_string(),
                language: "fr".to_string(),
                fan_out: true,
                allowed_languages: Vec::new(),
                min_confidence: 0.0,
            },
            audio: AudioConfig {
                vad_threshold: 0.05,
                ..AudioConfig::default()
            },
            ..Config::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        config.save(&path).unwrap();

        Config::update_model(&path, "medium").unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.stt.model, "medium");
        assert_eq!(reloaded.stt.language, "fr"); // preserved
        assert!(reloaded.stt.fan_out); // preserved
        assert_eq!(reloaded.audio.vad_threshold, 0.05); // preserved
    }

    #[test]
    fn test_update_model_creates_config_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subdir").join("config.toml");

        Config::update_model(&path, "small.en").unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.stt.model, "small.en");
        assert_eq!(reloaded.stt.language, "auto"); // default
    }

    #[test]
    fn test_save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("config.toml");

        let config = Config::default();
        config.save(&path).unwrap();

        assert!(path.exists());
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded, Config::default());
    }

    // ── Hallucination filter tests ──────────────────────────────────────

    #[test]
    fn test_default_hallucination_filters_has_expected_languages() {
        let defaults = default_hallucination_filters();
        let expected_langs = [
            "en", "de", "es", "fr", "pt", "it", "ru", "ja", "zh", "ko", "nl", "pl", "ar", "tr",
        ];
        for lang in &expected_langs {
            assert!(
                defaults.contains_key(*lang),
                "Missing language key: {}",
                lang
            );
            assert!(
                !defaults[*lang].is_empty(),
                "Language {} has no defaults",
                lang
            );
        }
        assert_eq!(defaults.len(), expected_langs.len());
    }

    #[test]
    fn test_transcription_config_omitted_uses_empty_defaults() {
        let toml_content = r#"
            [stt]
            model = "small.en"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.transcription.hallucination_filters.add.is_empty());
        assert!(
            config
                .transcription
                .hallucination_filters
                .overrides
                .is_empty()
        );
    }

    #[test]
    fn test_resolve_filters_no_overrides() {
        let config = HallucinationFilterConfig::default();
        let resolved = resolve_hallucination_filters(&config);
        // Should contain all built-in defaults from all languages
        assert!(!resolved.is_empty());
        // Verify at least one phrase from each of several languages
        assert!(resolved.contains(&"thank you".to_string()));
        assert!(resolved.contains(&"vielen dank".to_string()));
        assert!(resolved.contains(&"gracias".to_string()));
        assert!(resolved.contains(&"merci".to_string()));
    }

    #[test]
    fn test_resolve_filters_add_merges() {
        let config = HallucinationFilterConfig {
            add: vec!["Custom Phrase".to_string()],
            suspect_add: vec![],
            overrides: HashMap::new(),
        };
        let resolved = resolve_hallucination_filters(&config);
        assert!(resolved.contains(&"custom phrase".to_string()));
        // Built-in defaults should still be present
        assert!(resolved.contains(&"thank you".to_string()));
    }

    #[test]
    fn test_resolve_filters_override_replaces_language() {
        let mut overrides = HashMap::new();
        overrides.insert("en".to_string(), vec!["Only This".to_string()]);
        let config = HallucinationFilterConfig {
            add: vec![],
            suspect_add: vec![],
            overrides,
        };
        let resolved = resolve_hallucination_filters(&config);
        // Should have the override
        assert!(resolved.contains(&"only this".to_string()));
        // Should NOT have the original English defaults
        assert!(!resolved.contains(&"thank you".to_string()));
        // Other languages still present
        assert!(resolved.contains(&"vielen dank".to_string()));
    }

    #[test]
    fn test_resolve_filters_override_empty_disables_language() {
        let mut overrides = HashMap::new();
        overrides.insert("ko".to_string(), vec![]);
        let config = HallucinationFilterConfig {
            add: vec![],
            suspect_add: vec![],
            overrides,
        };
        let resolved = resolve_hallucination_filters(&config);
        // Korean defaults should be gone
        assert!(!resolved.contains(&"감사합니다".to_string()));
        // Others still present
        assert!(resolved.contains(&"thank you".to_string()));
    }

    #[test]
    fn test_resolve_filters_add_plus_override() {
        let mut overrides = HashMap::new();
        overrides.insert("en".to_string(), vec!["Custom English".to_string()]);
        let config = HallucinationFilterConfig {
            add: vec!["Extra".to_string()],
            suspect_add: vec![],
            overrides,
        };
        let resolved = resolve_hallucination_filters(&config);
        assert!(resolved.contains(&"custom english".to_string()));
        assert!(resolved.contains(&"extra".to_string()));
        assert!(!resolved.contains(&"thank you".to_string()));
        assert!(resolved.contains(&"vielen dank".to_string()));
    }

    #[test]
    fn test_get_value_by_path_string() {
        let config = Config::default();
        let value = config.get_value_by_path("stt.model").unwrap();
        assert_eq!(value, "base");
    }

    #[test]
    fn test_get_value_by_path_integer() {
        let config = Config::default();
        let value = config.get_value_by_path("audio.sample_rate").unwrap();
        assert_eq!(value, "16000");
    }

    #[test]
    fn test_get_value_by_path_invalid() {
        let config = Config::default();
        let result = config.get_value_by_path("nonexistent.key");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found"),
            "Error should mention 'not found': {}",
            err_msg
        );
    }

    #[test]
    fn test_set_value_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Create a config file first
        let config = Config::default();
        config.save(&path).unwrap();

        // Set a value
        Config::set_value_by_path(&path, "stt.model", "small.en").unwrap();

        // Verify it was set
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(reloaded.stt.model, "small.en");
        // Other values preserved
        assert_eq!(reloaded.audio.sample_rate, 16000);
    }

    #[test]
    fn test_to_display_toml_roundtrip() {
        let config = Config::default();
        let toml_str = config.to_display_toml().unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_dump_template_contains_sections() {
        let template = Config::dump_template();
        assert!(template.contains("[audio]"), "Missing [audio] section");
        assert!(template.contains("[stt]"), "Missing [stt] section");
        assert!(
            template.contains("[injection]"),
            "Missing [injection] section"
        );
        assert!(
            template.contains("[voice_commands]"),
            "Missing [voice_commands] section"
        );
        assert!(
            template.contains("[transcription.hallucination_filters]"),
            "Missing [transcription.hallucination_filters] section"
        );
    }

    #[test]
    fn test_dump_template_contains_hallucination_defaults() {
        let template = Config::dump_template();
        assert!(
            template.contains("\"Thank you\""),
            "Missing English hallucination default"
        );
        assert!(
            template.contains("\"Vielen Dank\""),
            "Missing German hallucination default"
        );
        assert!(
            template.contains("\"Gracias\""),
            "Missing Spanish hallucination default"
        );
        assert!(
            template.contains("\"Merci\""),
            "Missing French hallucination default"
        );
    }

    #[test]
    fn test_dump_template_contains_disable_defaults() {
        let template = Config::dump_template();
        assert!(
            template.contains("disable_defaults"),
            "Missing disable_defaults field"
        );
        assert!(
            template.contains("disable_defaults = false"),
            "Missing disable_defaults default value"
        );
        assert!(
            template.contains("Set to true to disable all built-in commands"),
            "Missing disable_defaults comment"
        );
    }

    #[test]
    fn test_dump_template_contains_builtin_commands_display() {
        let template = Config::dump_template();
        assert!(
            template.contains("Built-in commands (active unless disable_defaults = true)"),
            "Missing built-in commands header"
        );
        assert!(
            template.contains("English (en):"),
            "Missing English built-in commands"
        );
        assert!(
            template.contains("German (de):"),
            "Missing German built-in commands"
        );
        // Should contain at least some command examples
        assert!(
            template.contains("period") || template.contains("punkt"),
            "Missing voice command examples"
        );
    }

    // ── display_section tests ─────────────────────────────────────────

    #[test]
    fn test_display_section_stt() {
        let config = Config::default();
        let output = config.display_section("stt").unwrap();
        assert!(
            output.contains("base"),
            "stt section should contain model name 'base': {}",
            output
        );
        assert!(
            output.contains("auto"),
            "stt section should contain language 'auto': {}",
            output
        );
    }

    #[test]
    fn test_display_section_audio() {
        let config = Config::default();
        let output = config.display_section("audio").unwrap();
        assert!(
            output.contains("16000"),
            "audio section should contain sample_rate: {}",
            output
        );
    }

    #[test]
    fn test_display_section_invalid_key() {
        let config = Config::default();
        let result = config.display_section("nonexistent");
        assert!(result.is_err());
    }

    // ── display_voice_commands tests ────────────────────────────────────

    #[test]
    fn test_display_voice_commands_korean() {
        let output = Config::display_voice_commands(&["ko"], &HashMap::new());
        assert!(
            output.contains("Korean"),
            "Should show Korean language name: {}",
            output
        );
        assert!(
            output.contains("마침표"),
            "Should contain Korean period command: {}",
            output
        );
        assert!(
            output.contains("\".\""),
            "Should show period replacement: {}",
            output
        );
    }

    #[test]
    fn test_display_voice_commands_english() {
        let output = Config::display_voice_commands(&["en"], &HashMap::new());
        assert!(
            output.contains("English"),
            "Should show English language name: {}",
            output
        );
        assert!(
            output.contains("period"),
            "Should contain period command: {}",
            output
        );
    }

    #[test]
    fn test_display_voice_commands_multiple_languages() {
        let output = Config::display_voice_commands(&["en", "de"], &HashMap::new());
        assert!(
            output.contains("English"),
            "Should show English: {}",
            output
        );
        assert!(output.contains("German"), "Should show German: {}", output);
    }

    #[test]
    fn test_display_voice_commands_with_custom() {
        let mut custom = HashMap::new();
        custom.insert("smiley".to_string(), ":)".to_string());
        let output = Config::display_voice_commands(&["en"], &custom);
        assert!(
            output.contains("Custom commands"),
            "Should show custom section: {}",
            output
        );
        assert!(
            output.contains("smiley"),
            "Should contain custom command: {}",
            output
        );
    }

    // ── validate_languages tests ────────────────────────────────────────

    #[test]
    fn test_validate_languages_valid() {
        assert!(Config::validate_languages(&["en"]).is_ok());
        assert!(Config::validate_languages(&["en", "de", "ko"]).is_ok());
    }

    #[test]
    fn test_validate_languages_invalid() {
        let result = Config::validate_languages(&["xx"]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("xx"),
            "Error should mention the bad language: {}",
            err_msg
        );
        assert!(
            err_msg.contains("Supported"),
            "Error should list supported languages: {}",
            err_msg
        );
    }

    #[test]
    fn test_validate_languages_mixed_valid_invalid() {
        let result = Config::validate_languages(&["en", "zz"]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("zz"),
            "Error should mention the bad language: {}",
            err_msg
        );
    }

    #[test]
    fn test_voice_commands_config_disable_defaults() {
        let toml_content = r#"
            [voice_commands]
            enabled = true
            disable_defaults = true
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.voice_commands.enabled);
        assert!(config.voice_commands.disable_defaults);
    }

    #[test]
    fn test_injection_backend_serializes_lowercase() {
        let config = Config {
            injection: InjectionConfig {
                method: InjectionMethod::Clipboard,
                paste_key: "auto".to_string(),
                backend: InjectionBackend::Portal,
            },
            ..Config::default()
        };

        let toml_str = toml::to_string(&config).unwrap();
        // Should serialize as lowercase "portal" not "Portal"
        assert!(
            toml_str.contains("backend = \"portal\""),
            "Backend should serialize as lowercase, got: {toml_str}"
        );
        assert!(
            !toml_str.contains("backend = \"Portal\""),
            "Backend should not serialize as capitalized"
        );
    }

    #[test]
    fn test_stt_config_with_allowed_languages() {
        let toml_content = r#"
            [stt]
            model = "base"
            language = "auto"
            allowed_languages = ["en", "de"]
            min_confidence = 0.5
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert_eq!(
            config.stt.allowed_languages,
            vec!["en".to_string(), "de".to_string()]
        );
        assert_eq!(config.stt.min_confidence, 0.5);
    }

    #[test]
    fn test_stt_config_allowed_languages_default_empty() {
        let config = Config::default();
        assert!(config.stt.allowed_languages.is_empty());
        assert_eq!(config.stt.min_confidence, 0.0);
    }

    // ── Suspect phrase tests ──────────────────────────────────────────

    #[test]
    fn test_default_suspect_phrases_loaded() {
        let defaults = default_suspect_phrases();
        let expected_langs = [
            "en", "de", "es", "fr", "pt", "it", "ru", "ja", "zh", "ko", "nl", "pl", "ar", "tr",
        ];
        for lang in &expected_langs {
            assert!(
                defaults.contains_key(*lang),
                "Missing suspect phrases for language: {}",
                lang
            );
            assert!(
                !defaults[*lang].is_empty(),
                "Language {} has no suspect phrases",
                lang
            );
        }
        // Verify specific entries
        assert!(
            defaults["en"].contains(&"okay".to_string()),
            "English suspect phrases should contain 'okay'"
        );
        assert!(
            defaults["de"].contains(&"ja".to_string()),
            "German suspect phrases should contain 'ja'"
        );
    }

    #[test]
    fn test_resolve_suspect_phrases() {
        let config = HallucinationFilterConfig::default();
        let resolved = resolve_suspect_phrases(&config);
        assert!(
            !resolved.is_empty(),
            "Resolved suspect phrases should not be empty"
        );
        // All should be lowercased
        for phrase in &resolved {
            assert_eq!(
                *phrase,
                phrase.to_lowercase(),
                "Suspect phrase '{}' should be lowercased",
                phrase
            );
        }
        // Should contain phrases from multiple languages
        assert!(
            resolved.contains(&"okay".to_string()),
            "Should contain 'okay' from English"
        );
        assert!(
            resolved.contains(&"ja".to_string()),
            "Should contain 'ja' from German"
        );
    }

    #[test]
    fn test_resolve_suspect_phrases_with_additions() {
        let config = HallucinationFilterConfig {
            add: vec![],
            suspect_add: vec!["Custom Filler".to_string()],
            overrides: HashMap::new(),
        };
        let resolved = resolve_suspect_phrases(&config);
        assert!(
            resolved.contains(&"custom filler".to_string()),
            "Should contain user-added suspect phrase (lowercased)"
        );
        // Built-in defaults still present
        assert!(
            resolved.contains(&"okay".to_string()),
            "Built-in suspect phrases should still be present"
        );
    }

    // ── Error correction config tests ─────────────────────────────────

    #[test]
    fn test_error_correction_config_defaults() {
        let config = ErrorCorrectionConfig::default();
        assert!(config.enabled, "Should be enabled by default");
        assert_eq!(config.backend, CorrectionBackend::Hybrid);
        assert_eq!(config.model, "flan-t5-base");
        assert_eq!(config.confidence_threshold, 0.65);
        assert_eq!(
            config.symspell_languages,
            vec!["he", "ar", "zh", "ja", "ko"]
        );
    }

    #[test]
    fn test_error_correction_config_from_toml() {
        let toml_content = r#"
            [transcription.error_correction]
            enabled = true
            backend = "t5"
            model = "flan-t5-base"
            confidence_threshold = 0.5
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.transcription.error_correction.enabled);
        assert_eq!(
            config.transcription.error_correction.backend,
            CorrectionBackend::T5
        );
        assert_eq!(config.transcription.error_correction.model, "flan-t5-base");
        assert_eq!(
            config.transcription.error_correction.confidence_threshold,
            0.5
        );
    }

    #[test]
    fn test_error_correction_config_omitted_uses_defaults() {
        let toml_content = r#"
            [stt]
            model = "small.en"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert!(config.transcription.error_correction.enabled);
        assert_eq!(
            config.transcription.error_correction.backend,
            CorrectionBackend::Hybrid
        );
        assert_eq!(config.transcription.error_correction.model, "flan-t5-base");
        assert_eq!(
            config.transcription.error_correction.confidence_threshold,
            0.65
        );
    }

    #[test]
    fn test_error_correction_get_value_by_path() {
        let config = Config::default();
        let enabled = config
            .get_value_by_path("transcription.error_correction.enabled")
            .unwrap();
        assert_eq!(enabled, "true");

        let model = config
            .get_value_by_path("transcription.error_correction.model")
            .unwrap();
        assert_eq!(model, "flan-t5-base");

        let threshold: f64 = config
            .get_value_by_path("transcription.error_correction.confidence_threshold")
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (threshold - 0.65).abs() < 0.001,
            "threshold should be ~0.65, got {threshold}"
        );
    }

    #[test]
    fn test_error_correction_set_value_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().save(&path).unwrap();

        Config::set_value_by_path(&path, "transcription.error_correction.enabled", "true").unwrap();
        let reloaded = Config::load(&path).unwrap();
        assert!(reloaded.transcription.error_correction.enabled);

        Config::set_value_by_path(
            &path,
            "transcription.error_correction.model",
            "flan-t5-large",
        )
        .unwrap();
        let reloaded = Config::load(&path).unwrap();
        assert_eq!(
            reloaded.transcription.error_correction.model,
            "flan-t5-large"
        );
    }

    #[test]
    fn test_error_correction_in_dump_template() {
        let template = Config::dump_template();
        assert!(
            template.contains("[transcription.error_correction]"),
            "Missing [transcription.error_correction] section"
        );
        assert!(
            template.contains("symspell"),
            "Missing symspell backend in template"
        );
        assert!(
            template.contains("flan-t5-base"),
            "Missing default model in template"
        );
        assert!(
            template.contains("confidence_threshold"),
            "Missing confidence_threshold in template"
        );
    }

    #[test]
    fn test_error_correction_roundtrip() {
        let config = Config {
            transcription: TranscriptionConfig {
                error_correction: ErrorCorrectionConfig {
                    enabled: true,
                    backend: CorrectionBackend::T5,
                    model: "flan-t5-large".to_string(),
                    confidence_threshold: 0.5,
                    dictionary_language: "auto".to_string(),
                    symspell_languages: Default::default(),
                },
                ..TranscriptionConfig::default()
            },
            ..Config::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        config.save(&path).unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(
            reloaded.transcription.error_correction,
            config.transcription.error_correction
        );
    }

    #[test]
    fn test_correction_backend_from_toml() {
        // Test loading symspell backend
        let toml = r#"
            [transcription.error_correction]
            backend = "symspell"
        "#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml.as_bytes()).unwrap();
        let config = Config::load(temp_file.path()).unwrap();
        assert_eq!(
            config.transcription.error_correction.backend,
            CorrectionBackend::Symspell
        );

        // Test loading t5 backend
        let toml = r#"
            [transcription.error_correction]
            backend = "t5"
        "#;
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml.as_bytes()).unwrap();
        let config = Config::load(temp_file.path()).unwrap();
        assert_eq!(
            config.transcription.error_correction.backend,
            CorrectionBackend::T5
        );
    }

    #[test]
    fn test_correction_backend_default_is_hybrid() {
        let config = Config::default();
        assert_eq!(
            config.transcription.error_correction.backend,
            CorrectionBackend::Hybrid
        );
    }

    #[test]
    fn test_error_correction_config_dictionary_language_default() {
        let config = ErrorCorrectionConfig::default();
        assert_eq!(config.dictionary_language, "auto");
    }

    #[test]
    fn test_error_correction_config_dictionary_language_from_toml() {
        let toml_content = r#"
            [transcription.error_correction]
            enabled = true
            backend = "symspell"
            dictionary_language = "de"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(temp_file.path()).unwrap();
        assert_eq!(
            config.transcription.error_correction.dictionary_language,
            "de"
        );
    }

    #[test]
    fn test_error_correction_config_dictionary_language_roundtrip() {
        let config = Config {
            transcription: TranscriptionConfig {
                error_correction: ErrorCorrectionConfig {
                    enabled: true,
                    backend: CorrectionBackend::Symspell,
                    model: "flan-t5-base".to_string(),
                    confidence_threshold: 0.85,
                    dictionary_language: "fr".to_string(),
                    symspell_languages: Default::default(),
                },
                ..TranscriptionConfig::default()
            },
            ..Config::default()
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        config.save(&path).unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(
            reloaded.transcription.error_correction.dictionary_language,
            "fr"
        );
    }

    #[test]
    fn test_dump_template_contains_dictionary_language() {
        let template = Config::dump_template();
        assert!(
            template.contains("dictionary_language"),
            "Missing dictionary_language field in template"
        );
        assert!(
            template.contains("auto"),
            "Missing 'auto' default value in template"
        );
        assert!(
            template.contains("en, de, es, fr, he, it, ru"),
            "Missing language list in template"
        );
    }
}
