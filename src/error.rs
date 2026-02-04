//! Error types for voicsh.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum VoicshError {
    // Configuration errors
    #[error("Configuration file not found at {path}")]
    ConfigFileNotFound { path: String },

    #[error("Failed to parse configuration: {message}")]
    ConfigParse { message: String },

    #[error("Invalid configuration value for {key}: {message}")]
    ConfigInvalidValue { key: String, message: String },

    #[error("Configuration error: {0}")]
    Config(#[from] toml::de::Error),

    // Audio capture errors
    #[error("Audio device not found: {device}")]
    AudioDeviceNotFound { device: String },

    #[error("Audio format mismatch: expected {expected}, got {actual}")]
    AudioFormatMismatch { expected: String, actual: String },

    #[error("Audio capture failed: {message}")]
    AudioCapture { message: String },

    // Transcription errors
    #[error("Transcription model not found at {path}")]
    TranscriptionModelNotFound { path: String },

    #[error("Transcription inference failed: {message}")]
    TranscriptionInferenceFailed { message: String },

    #[error("Transcription error: {message}")]
    Transcription { message: String },

    // Text injection errors
    #[error("Text injection tool not found: {tool}")]
    InjectionToolNotFound { tool: String },

    #[error("Text injection permission denied: {message}")]
    InjectionPermissionDenied { message: String },

    #[error("Text injection failed: {message}")]
    InjectionFailed { message: String },

    // IPC errors
    #[error("IPC socket error: {message}")]
    IpcSocket { message: String },

    #[error("IPC protocol error: {message}")]
    IpcProtocol { message: String },

    #[error("IPC connection failed: {message}")]
    IpcConnection { message: String },

    // General I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    // Generic error for cases not covered above
    #[error("{0}")]
    Other(String),
}

// Type alias for convenience
pub type Result<T> = std::result::Result<T, VoicshError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_config_file_not_found_display() {
        let error = VoicshError::ConfigFileNotFound {
            path: "/path/to/config.toml".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Configuration file not found at /path/to/config.toml"
        );
    }

    #[test]
    fn test_config_parse_display() {
        let error = VoicshError::ConfigParse {
            message: "invalid TOML syntax".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Failed to parse configuration: invalid TOML syntax"
        );
    }

    #[test]
    fn test_config_invalid_value_display() {
        let error = VoicshError::ConfigInvalidValue {
            key: "sample_rate".to_string(),
            message: "must be positive".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Invalid configuration value for sample_rate: must be positive"
        );
    }

    #[test]
    fn test_audio_device_not_found_display() {
        let error = VoicshError::AudioDeviceNotFound {
            device: "default".to_string(),
        };
        assert_eq!(error.to_string(), "Audio device not found: default");
    }

    #[test]
    fn test_audio_format_mismatch_display() {
        let error = VoicshError::AudioFormatMismatch {
            expected: "16kHz mono".to_string(),
            actual: "44.1kHz stereo".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Audio format mismatch: expected 16kHz mono, got 44.1kHz stereo"
        );
    }

    #[test]
    fn test_audio_capture_display() {
        let error = VoicshError::AudioCapture {
            message: "buffer overflow".to_string(),
        };
        assert_eq!(error.to_string(), "Audio capture failed: buffer overflow");
    }

    #[test]
    fn test_transcription_model_not_found_display() {
        let error = VoicshError::TranscriptionModelNotFound {
            path: "/models/whisper.bin".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Transcription model not found at /models/whisper.bin"
        );
    }

    #[test]
    fn test_transcription_inference_failed_display() {
        let error = VoicshError::TranscriptionInferenceFailed {
            message: "out of memory".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Transcription inference failed: out of memory"
        );
    }

    #[test]
    fn test_transcription_display() {
        let error = VoicshError::Transcription {
            message: "invalid audio format".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Transcription error: invalid audio format"
        );
    }

    #[test]
    fn test_injection_tool_not_found_display() {
        let error = VoicshError::InjectionToolNotFound {
            tool: "xdotool".to_string(),
        };
        assert_eq!(error.to_string(), "Text injection tool not found: xdotool");
    }

    #[test]
    fn test_injection_permission_denied_display() {
        let error = VoicshError::InjectionPermissionDenied {
            message: "X11 access denied".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Text injection permission denied: X11 access denied"
        );
    }

    #[test]
    fn test_injection_failed_display() {
        let error = VoicshError::InjectionFailed {
            message: "window not found".to_string(),
        };
        assert_eq!(error.to_string(), "Text injection failed: window not found");
    }

    #[test]
    fn test_ipc_socket_display() {
        let error = VoicshError::IpcSocket {
            message: "bind failed".to_string(),
        };
        assert_eq!(error.to_string(), "IPC socket error: bind failed");
    }

    #[test]
    fn test_ipc_protocol_display() {
        let error = VoicshError::IpcProtocol {
            message: "invalid message format".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "IPC protocol error: invalid message format"
        );
    }

    #[test]
    fn test_ipc_connection_display() {
        let error = VoicshError::IpcConnection {
            message: "timeout".to_string(),
        };
        assert_eq!(error.to_string(), "IPC connection failed: timeout");
    }

    #[test]
    fn test_other_display() {
        let error = VoicshError::Other("unexpected error".to_string());
        assert_eq!(error.to_string(), "unexpected error");
    }

    #[test]
    fn test_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let error: VoicshError = io_error.into();
        assert!(error.to_string().contains("file not found"));
    }

    #[test]
    fn test_from_toml_error() {
        let toml_str = "invalid = toml = syntax";
        let toml_error = toml::from_str::<toml::Value>(toml_str).unwrap_err();
        let error: VoicshError = toml_error.into();
        assert!(error.to_string().contains("Configuration error"));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }
        assert_eq!(returns_result().unwrap(), 42);

        fn returns_error() -> Result<i32> {
            Err(VoicshError::Other("test error".to_string()))
        }
        assert!(returns_error().is_err());
    }

    #[test]
    fn test_error_source_chain_io() {
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let error: VoicshError = io_error.into();

        // Test that the error can be used with std::error::Error trait
        let error_trait: &dyn std::error::Error = &error;
        assert!(error_trait.source().is_some());
    }

    #[test]
    fn test_error_source_chain_toml() {
        let toml_str = "key = 'unclosed string";
        let toml_error = toml::from_str::<toml::Value>(toml_str).unwrap_err();
        let error: VoicshError = toml_error.into();

        // Test that the error can be used with std::error::Error trait
        let error_trait: &dyn std::error::Error = &error;
        assert!(error_trait.source().is_some());
    }

    #[test]
    fn test_error_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<VoicshError>();
        assert_sync::<VoicshError>();
    }

    #[test]
    fn test_error_debug_format() {
        let error = VoicshError::ConfigFileNotFound {
            path: "/test/path".to_string(),
        };
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("ConfigFileNotFound"));
        assert!(debug_str.contains("/test/path"));
    }
}
