use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    pub stt: SttConfig,
    pub input: InputConfig,
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
}

/// Input method configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InputConfig {
    pub method: InputMethod,
    pub paste_key: String,
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
            sample_rate: 16000,
            vad_threshold: 0.02,
            silence_duration_ms: 1500,
        }
    }
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model: "base.en".to_string(),
            language: "en".to_string(),
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            method: InputMethod::Clipboard,
            paste_key: "ctrl+v".to_string(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    ///
    /// Returns an error if the file contains invalid TOML.
    /// Missing fields will use default values.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
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
            Err(e) => {
                if e.downcast_ref::<std::io::Error>()
                    .map(|io_err| io_err.kind() == std::io::ErrorKind::NotFound)
                    .unwrap_or(false)
                {
                    Self::default()
                } else {
                    // Re-panic on invalid TOML or other errors
                    panic!("Failed to load config from {}: {}", path.display(), e);
                }
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
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .expect("Could not determine config directory")
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
        assert_eq!(config.stt.model, "base.en");
        assert_eq!(config.stt.language, "en");

        // Input defaults
        assert_eq!(config.input.method, InputMethod::Clipboard);
        assert_eq!(config.input.paste_key, "ctrl+v");
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
        assert_eq!(config.stt.language, "en");
        assert_eq!(config.input.method, InputMethod::Clipboard);
        assert_eq!(config.input.paste_key, "ctrl+v");
    }

    #[test]
    fn test_env_override_model() {
        let _lock = ENV_LOCK.lock().unwrap();
        clear_voicsh_env();

        set_env("VOICSH_MODEL", "tiny.en");
        let config = Config::default().with_env_overrides();

        assert_eq!(config.stt.model, "tiny.en");
        assert_eq!(config.stt.language, "en"); // Not overridden

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
        assert_eq!(config.stt.model, "base.en");

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
}
