use crate::defaults;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
#[cfg(feature = "cli")]
use std::path::PathBuf;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    pub stt: SttConfig,
    pub input: InputConfig,
    pub voice_commands: VoiceCommandConfig,
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
}

/// Input method configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InputConfig {
    pub method: InputMethod,
    pub paste_key: String,
}

/// Voice command configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VoiceCommandConfig {
    /// Enable voice command processing (default: true)
    pub enabled: bool,
    /// User-defined command overrides: spoken phrase → replacement text
    pub commands: std::collections::HashMap<String, String>,
}

impl Default for VoiceCommandConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            commands: std::collections::HashMap::new(),
        }
    }
}

/// Input method enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InputMethod {
    Clipboard,
    Direct,
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
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            method: InputMethod::Clipboard,
            paste_key: "auto".to_string(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    // Mutex to serialize tests that modify environment variables
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // SAFETY: These helpers are only used in tests with ENV_LOCK held,
    // ensuring no concurrent access to environment variables.
    fn set_env(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) }
    }

    fn remove_env(key: &str) {
        unsafe { std::env::remove_var(key) }
    }

    fn clear_voicsh_env() {
        remove_env("VOICSH_MODEL");
        remove_env("VOICSH_LANGUAGE");
        remove_env("VOICSH_AUDIO_DEVICE");
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

        // Input defaults
        assert_eq!(config.input.method, InputMethod::Clipboard);
        assert_eq!(config.input.paste_key, "auto");
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

            [input]
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

        assert_eq!(config.input.method, InputMethod::Direct);
        assert_eq!(config.input.paste_key, "ctrl+shift+v");
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
        assert_eq!(config.input.method, InputMethod::Clipboard);
        assert_eq!(config.input.paste_key, "auto");
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
}
