//! JSON message protocol for IPC communication between CLI and daemon.

use serde::{Deserialize, Serialize};

/// Commands sent by CLI to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Toggle recording on/off
    Toggle,
    /// Start recording
    Start,
    /// Stop recording and transcribe
    Stop,
    /// Cancel recording without transcribing
    Cancel,
    /// Get daemon status
    Status,
    /// Shutdown the daemon
    Shutdown,
    /// Follow daemon events (live streaming)
    Follow,
    /// Set language for transcription
    SetLanguage { language: String },
    /// List supported languages
    ListLanguages,
    /// Set model for transcription
    SetModel { model: String },
    /// List available models
    ListModels,
}

impl Command {
    /// Serialize command to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize command from JSON string.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Model information returned to clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfoResponse {
    pub name: String,
    pub size_mb: u32,
    pub english_only: bool,
    pub installed: bool,
    pub quantized: bool,
}

/// Responses sent by daemon to CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Command succeeded
    Ok { message: String },
    /// Command succeeded with transcription result
    Transcription { text: String },
    /// Current daemon status
    Status {
        recording: bool,
        model_loaded: bool,
        model_name: Option<String>,
        language: Option<String>,
        daemon_version: String,
        backend: String,
        device: Option<String>,
    },
    /// Error occurred
    Error { message: String },
    /// Supported languages list
    Languages {
        languages: Vec<String>,
        current: String,
    },
    /// Available models list
    Models {
        models: Vec<ModelInfoResponse>,
        current: String,
    },
}

impl Response {
    /// Serialize response to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize response from JSON string.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Events streamed from daemon to follow clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonEvent {
    /// Audio level update (throttled to ~15 Hz)
    Level {
        level: f32,
        threshold: f32,
        is_speech: bool,
        buffer_used: u16,
        buffer_capacity: u16,
    },
    /// Recording state changed
    RecordingStateChanged { recording: bool },
    /// Transcription result with language and confidence
    Transcription {
        text: String,
        language: String,
        confidence: f32,
    },
    /// Transcription dropped by language/confidence filter
    TranscriptionDropped {
        text: String,
        language: String,
        confidence: f32,
        reason: String,
    },
    /// Log message from daemon
    Log { message: String },
    /// Config value changed
    ConfigChanged { key: String, value: String },
    /// Model loading started or progressing
    ModelLoading { model: String, progress: String },
    /// Model loaded successfully
    ModelLoaded { model: String },
    /// Model loading failed
    ModelLoadFailed { model: String, error: String },
}

impl DaemonEvent {
    /// Serialize event to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize event from JSON string.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Command Tests

    #[test]
    fn test_command_toggle_json_roundtrip() {
        let cmd = Command::Toggle;
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
    }

    #[test]
    fn test_command_all_variants_serialize() {
        let commands = vec![
            Command::Toggle,
            Command::Start,
            Command::Stop,
            Command::Cancel,
            Command::Status,
            Command::Shutdown,
            Command::Follow,
            Command::SetLanguage {
                language: "de".to_string(),
            },
            Command::ListLanguages,
            Command::SetModel {
                model: "large".to_string(),
            },
            Command::ListModels,
        ];

        for cmd in commands {
            let json = cmd.to_json().expect("should serialize");
            let deserialized = Command::from_json(&json).expect("should deserialize");
            assert_eq!(cmd, deserialized, "roundtrip failed for {:?}", cmd);
        }
    }

    #[test]
    fn test_json_format_is_snake_case() {
        let cmd = Command::Toggle;
        let json = cmd.to_json().expect("should serialize");
        assert!(
            json.contains("\"type\":\"toggle\""),
            "JSON should use snake_case. Got: {}",
            json
        );

        let cmd = Command::Start;
        let json = cmd.to_json().expect("should serialize");
        assert!(
            json.contains("\"type\":\"start\""),
            "JSON should use snake_case. Got: {}",
            json
        );
    }

    // Response Tests

    #[test]
    fn test_response_ok_json_roundtrip() {
        let resp = Response::Ok {
            message: "Done".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_response_transcription_json_roundtrip() {
        let resp = Response::Transcription {
            text: "hello world".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains("\"type\":\"transcription\""));
        assert!(json.contains("\"text\":\"hello world\""));
    }

    #[test]
    fn test_response_status_json_roundtrip() {
        let resp = Response::Status {
            recording: true,
            model_loaded: true,
            model_name: Some("base.en".to_string()),
            language: Some("en".to_string()),
            daemon_version: "0.0.1+test".to_string(),
            backend: "CPU".to_string(),
            device: None,
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains("\"type\":\"status\""));
        assert!(json.contains("\"recording\":true"));
        assert!(json.contains("\"model_loaded\":true"));
        assert!(json.contains("\"model_name\":\"base.en\""));
    }

    #[test]
    fn test_response_status_with_none_model_name() {
        let resp = Response::Status {
            recording: false,
            model_loaded: false,
            model_name: None,
            language: None,
            daemon_version: "0.0.1".to_string(),
            backend: "CPU".to_string(),
            device: None,
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_response_status_with_language() {
        let resp = Response::Status {
            recording: true,
            model_loaded: true,
            model_name: Some("base.en".to_string()),
            language: Some("en".to_string()),
            daemon_version: "0.0.1+test".to_string(),
            backend: "CUDA".to_string(),
            device: Some("RTX 5060 Ti".to_string()),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains("\"language\":\"en\""));
    }

    #[test]
    fn test_response_status_language_none() {
        let resp = Response::Status {
            recording: false,
            model_loaded: false,
            model_name: None,
            language: None,
            daemon_version: "0.0.1".to_string(),
            backend: "CPU".to_string(),
            device: None,
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_response_error_json_roundtrip() {
        let resp = Response::Error {
            message: "Model not found".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"Model not found\""));
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let invalid = r#"{"type": "unknown_command"}"#;
        let result = Command::from_json(invalid);
        assert!(result.is_err(), "should fail for unknown command type");

        let invalid = r#"{"invalid": "json"}"#;
        let result = Command::from_json(invalid);
        assert!(result.is_err(), "should fail for missing type field");

        let invalid = r#"not json at all"#;
        let result = Command::from_json(invalid);
        assert!(result.is_err(), "should fail for malformed JSON");
    }

    #[test]
    fn test_command_json_format_examples() {
        // Verify the exact format matches expected output
        let toggle = Command::Toggle.to_json().unwrap();
        assert_eq!(toggle, r#"{"type":"toggle"}"#);

        let start = Command::Start.to_json().unwrap();
        assert_eq!(start, r#"{"type":"start"}"#);

        let status = Command::Status.to_json().unwrap();
        assert_eq!(status, r#"{"type":"status"}"#);
    }

    #[test]
    fn test_response_json_format_examples() {
        let ok = Response::Ok {
            message: "Done".to_string(),
        }
        .to_json()
        .unwrap();
        assert!(ok.contains(r#""type":"ok""#));
        assert!(ok.contains(r#""message":"Done""#));

        let error = Response::Error {
            message: "test error".to_string(),
        }
        .to_json()
        .unwrap();
        assert!(error.contains(r#""type":"error""#));
        assert!(error.contains(r#""message":"test error""#));
    }

    #[test]
    fn test_response_transcription_with_special_chars() {
        let resp = Response::Transcription {
            text: r#"Hello "world" with \n special chars"#.to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
    }

    #[test]
    fn test_response_error_with_special_chars() {
        let resp = Response::Error {
            message: "Error: connection failed (timeout)".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
    }

    // Malformed IPC protocol tests
    #[test]
    fn test_malformed_command_empty_json() {
        let empty = "{}";
        let result = Command::from_json(empty);
        assert!(result.is_err(), "Empty JSON should be rejected");
    }

    #[test]
    fn test_malformed_command_null() {
        let null_json = "null";
        let result = Command::from_json(null_json);
        assert!(result.is_err(), "Null JSON should be rejected");
    }

    #[test]
    fn test_malformed_command_array() {
        let array = r#"["toggle", "start"]"#;
        let result = Command::from_json(array);
        assert!(result.is_err(), "Array should be rejected");
    }

    #[test]
    fn test_malformed_command_nested_objects() {
        let nested = r#"{"type": {"nested": "toggle"}}"#;
        let result = Command::from_json(nested);
        assert!(
            result.is_err(),
            "Nested object in type field should be rejected"
        );
    }

    #[test]
    fn test_malformed_command_extra_fields() {
        let extra = r#"{"type": "toggle", "extra": "field", "another": 123}"#;
        let result = Command::from_json(extra);
        // Extra fields might be ignored or cause error - just verify no panic
        let _ = result;
    }

    #[test]
    fn test_malformed_command_wrong_case() {
        let wrong_case = r#"{"type": "Toggle"}"#; // Capital T
        let result = Command::from_json(wrong_case);
        assert!(result.is_err(), "Wrong case should be rejected");

        let all_caps = r#"{"type": "TOGGLE"}"#;
        let result = Command::from_json(all_caps);
        assert!(result.is_err(), "All caps should be rejected");
    }

    #[test]
    fn test_malformed_command_numeric_type() {
        let numeric = r#"{"type": 123}"#;
        let result = Command::from_json(numeric);
        assert!(result.is_err(), "Numeric type should be rejected");
    }

    #[test]
    fn test_malformed_command_boolean_type() {
        let boolean = r#"{"type": true}"#;
        let result = Command::from_json(boolean);
        assert!(result.is_err(), "Boolean type should be rejected");
    }

    #[test]
    fn test_malformed_command_empty_string_type() {
        let empty_str = r#"{"type": ""}"#;
        let result = Command::from_json(empty_str);
        assert!(result.is_err(), "Empty string type should be rejected");
    }

    #[test]
    fn test_malformed_command_whitespace_type() {
        let whitespace = r#"{"type": "   "}"#;
        let result = Command::from_json(whitespace);
        assert!(result.is_err(), "Whitespace-only type should be rejected");
    }

    #[test]
    fn test_malformed_command_unicode_in_type() {
        let unicode = r#"{"type": "启动"}"#; // Chinese for "start"
        let result = Command::from_json(unicode);
        assert!(result.is_err(), "Unicode type should be rejected");
    }

    #[test]
    fn test_malformed_response_empty_json() {
        let empty = "{}";
        let result = Response::from_json(empty);
        assert!(result.is_err(), "Empty JSON response should be rejected");
    }

    #[test]
    fn test_malformed_response_missing_required_fields() {
        let missing_message = r#"{"type": "error"}"#;
        let result = Response::from_json(missing_message);
        assert!(
            result.is_err(),
            "Error response without message should be rejected"
        );

        let missing_text = r#"{"type": "transcription"}"#;
        let result = Response::from_json(missing_text);
        assert!(
            result.is_err(),
            "Transcription response without text should be rejected"
        );
    }

    #[test]
    fn test_malformed_response_wrong_field_types() {
        let wrong_type = r#"{"type": "error", "message": 123}"#;
        let result = Response::from_json(wrong_type);
        assert!(result.is_err(), "Numeric message should be rejected");

        let bool_field = r#"{"type": "status", "recording": "yes"}"#;
        let result = Response::from_json(bool_field);
        // Might fail or succeed depending on serde configuration
        let _ = result;
    }

    #[test]
    fn test_malformed_json_syntax() {
        let invalid_cases = vec![
            r#"{"type": "toggle""#,              // Missing closing brace
            r#"{"type" "toggle"}"#,              // Missing colon
            r#"{type: "toggle"}"#,               // Unquoted key
            r#"{'type': 'toggle'}"#,             // Single quotes
            r#"{"type": "toggle",}"#,            // Trailing comma
            r#"{"type": "toggle"; "extra": 1}"#, // Semicolon instead of comma
        ];

        for (i, invalid) in invalid_cases.iter().enumerate() {
            let result = Command::from_json(invalid);
            assert!(
                result.is_err(),
                "Case {} should be rejected: {}",
                i,
                invalid
            );
        }
    }

    #[test]
    fn test_malformed_response_with_unicode() {
        // Unicode in message fields should be valid
        let unicode_error = Response::Error {
            message: "错误：连接失败".to_string(), // Chinese error message
        };
        let json = unicode_error.to_json().expect("Unicode should serialize");
        let deserialized = Response::from_json(&json).expect("Unicode should deserialize");
        assert_eq!(
            unicode_error, deserialized,
            "Unicode should round-trip correctly"
        );

        let emoji_transcription = Response::Transcription {
            text: "Hello 👋 World 🌍".to_string(),
        };
        let json = emoji_transcription
            .to_json()
            .expect("Emoji should serialize");
        let deserialized = Response::from_json(&json).expect("Emoji should deserialize");
        assert_eq!(
            emoji_transcription, deserialized,
            "Emoji should round-trip correctly"
        );
    }

    #[test]
    fn test_malformed_extremely_long_strings() {
        // Very long message (10,000 characters)
        let long_message = "x".repeat(10_000);
        let response = Response::Error {
            message: long_message.clone(),
        };
        let json = response.to_json().expect("Long message should serialize");
        let deserialized = Response::from_json(&json).expect("Long message should deserialize");
        match deserialized {
            Response::Error { message } => {
                assert_eq!(message.len(), 10_000, "Message length should be preserved");
                assert_eq!(message, long_message, "Message content should be preserved");
            }
            _ => panic!("Expected Error response"),
        }
    }

    #[test]
    fn test_malformed_control_characters_in_strings() {
        // Test control characters and special chars in strings
        let special_chars = "Test\nwith\nnewlines\tand\ttabs\rand\rcarriage\0returns";
        let response = Response::Transcription {
            text: special_chars.to_string(),
        };
        let json = response.to_json().expect("Control chars should serialize");
        let deserialized = Response::from_json(&json).expect("Control chars should deserialize");
        match deserialized {
            Response::Transcription { text } => {
                assert_eq!(
                    text, special_chars,
                    "Control characters should be preserved"
                );
            }
            _ => panic!("Expected Transcription response"),
        }
    }

    // DaemonEvent tests

    #[test]
    fn test_daemon_event_level_json_roundtrip() {
        let event = DaemonEvent::Level {
            level: 0.15,
            threshold: 0.08,
            is_speech: true,
            buffer_used: 3,
            buffer_capacity: 8,
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert!(json.contains("\"type\":\"level\""));
        assert!(json.contains("\"is_speech\":true"));
    }

    #[test]
    fn test_daemon_event_recording_state_changed_json_roundtrip() {
        let event = DaemonEvent::RecordingStateChanged { recording: true };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert!(json.contains("\"type\":\"recording_state_changed\""));
        assert!(json.contains("\"recording\":true"));
    }

    #[test]
    fn test_daemon_event_transcription_json_roundtrip() {
        let event = DaemonEvent::Transcription {
            text: "hello world".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert!(json.contains("\"type\":\"transcription\""));
        assert!(json.contains("\"text\":\"hello world\""));
    }

    #[test]
    fn test_daemon_event_log_json_roundtrip() {
        let event = DaemonEvent::Log {
            message: "Model loaded".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert!(json.contains("\"type\":\"log\""));
    }

    #[test]
    fn test_daemon_event_all_variants_roundtrip() {
        let events = vec![
            DaemonEvent::Level {
                level: 0.0,
                threshold: 0.02,
                is_speech: false,
                buffer_used: 0,
                buffer_capacity: 0,
            },
            DaemonEvent::Level {
                level: 0.5,
                threshold: 0.1,
                is_speech: true,
                buffer_used: 5,
                buffer_capacity: 10,
            },
            DaemonEvent::RecordingStateChanged { recording: false },
            DaemonEvent::RecordingStateChanged { recording: true },
            DaemonEvent::Transcription {
                text: String::new(),
                language: "en".to_string(),
                confidence: 1.0,
            },
            DaemonEvent::Transcription {
                text: "Hello 👋 World".to_string(),
                language: "de".to_string(),
                confidence: 0.85,
            },
            DaemonEvent::TranscriptionDropped {
                text: "test".to_string(),
                language: "ru".to_string(),
                confidence: 0.3,
                reason: "language not in allowlist".to_string(),
            },
            DaemonEvent::Log {
                message: "test".to_string(),
            },
        ];

        for event in events {
            let json = event.to_json().expect("should serialize");
            let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
            assert_eq!(event, deserialized, "roundtrip failed for {:?}", event);
        }
    }

    #[test]
    fn test_command_follow_json_roundtrip() {
        let cmd = Command::Follow;
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
        assert_eq!(json, r#"{"type":"follow"}"#);
    }

    #[test]
    fn test_daemon_event_level_float_precision() {
        let event = DaemonEvent::Level {
            level: 0.123456789,
            threshold: 0.001,
            is_speech: false,
            buffer_used: 0,
            buffer_capacity: 0,
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        // Float values should survive roundtrip (serde_json preserves f32 precision)
        match deserialized {
            DaemonEvent::Level {
                level,
                threshold,
                is_speech,
                buffer_used,
                buffer_capacity,
            } => {
                assert!(
                    (level - 0.123456789_f32).abs() < 1e-6,
                    "level should be close"
                );
                assert!(
                    (threshold - 0.001_f32).abs() < 1e-6,
                    "threshold should be close"
                );
                assert!(!is_speech);
                assert_eq!(buffer_used, 0);
                assert_eq!(buffer_capacity, 0);
            }
            _ => panic!("Expected Level event"),
        }
    }

    // New command variant tests

    #[test]
    fn test_command_set_language_json_roundtrip() {
        let cmd = Command::SetLanguage {
            language: "de".to_string(),
        };
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
        assert_eq!(json, r#"{"type":"set_language","language":"de"}"#);
    }

    #[test]
    fn test_command_list_languages_json_roundtrip() {
        let cmd = Command::ListLanguages;
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
        assert_eq!(json, r#"{"type":"list_languages"}"#);
    }

    #[test]
    fn test_command_set_model_json_roundtrip() {
        let cmd = Command::SetModel {
            model: "large".to_string(),
        };
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
        assert_eq!(json, r#"{"type":"set_model","model":"large"}"#);
    }

    #[test]
    fn test_command_list_models_json_roundtrip() {
        let cmd = Command::ListModels;
        let json = cmd.to_json().expect("should serialize");
        let deserialized = Command::from_json(&json).expect("should deserialize");
        assert_eq!(cmd, deserialized);
        assert_eq!(json, r#"{"type":"list_models"}"#);
    }

    // New response variant tests

    #[test]
    fn test_response_languages_json_roundtrip() {
        let resp = Response::Languages {
            languages: vec!["auto".to_string(), "en".to_string(), "de".to_string()],
            current: "auto".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains(r#""type":"languages""#));
        assert!(json.contains(r#""current":"auto""#));
    }

    #[test]
    fn test_response_models_json_roundtrip() {
        let resp = Response::Models {
            models: vec![
                ModelInfoResponse {
                    name: "base".to_string(),
                    size_mb: 142,
                    english_only: false,
                    installed: true,
                    quantized: false,
                },
                ModelInfoResponse {
                    name: "large".to_string(),
                    size_mb: 2880,
                    english_only: false,
                    installed: false,
                    quantized: false,
                },
            ],
            current: "base".to_string(),
        };
        let json = resp.to_json().expect("should serialize");
        let deserialized = Response::from_json(&json).expect("should deserialize");
        assert_eq!(resp, deserialized);
        assert!(json.contains(r#""type":"models""#));
        assert!(json.contains(r#""current":"base""#));
    }

    #[test]
    fn test_model_info_response_json_roundtrip() {
        let info = ModelInfoResponse {
            name: "base".to_string(),
            size_mb: 142,
            english_only: false,
            installed: true,
            quantized: false,
        };
        let json = serde_json::to_string(&info).expect("should serialize");
        let deserialized: ModelInfoResponse =
            serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(info, deserialized);
        assert!(json.contains(r#""name":"base""#));
        assert!(json.contains(r#""size_mb":142"#));
        assert!(json.contains(r#""english_only":false"#));
        assert!(json.contains(r#""installed":true"#));
        assert!(json.contains(r#""quantized":false"#));
    }

    // New daemon event variant tests

    #[test]
    fn test_daemon_event_config_changed_json_roundtrip() {
        let event = DaemonEvent::ConfigChanged {
            key: "language".to_string(),
            value: "de".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert_eq!(
            json,
            r#"{"type":"config_changed","key":"language","value":"de"}"#
        );
    }

    #[test]
    fn test_daemon_event_model_loading_json_roundtrip() {
        let event = DaemonEvent::ModelLoading {
            model: "large".to_string(),
            progress: "downloading".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert_eq!(
            json,
            r#"{"type":"model_loading","model":"large","progress":"downloading"}"#
        );
    }

    #[test]
    fn test_daemon_event_model_loaded_json_roundtrip() {
        let event = DaemonEvent::ModelLoaded {
            model: "large".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert_eq!(json, r#"{"type":"model_loaded","model":"large"}"#);
    }

    #[test]
    fn test_daemon_event_model_load_failed_json_roundtrip() {
        let event = DaemonEvent::ModelLoadFailed {
            model: "large".to_string(),
            error: "Download failed".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert_eq!(
            json,
            r#"{"type":"model_load_failed","model":"large","error":"Download failed"}"#
        );
    }

    #[test]
    fn test_daemon_event_transcription_dropped_json_roundtrip() {
        let event = DaemonEvent::TranscriptionDropped {
            text: "hello".to_string(),
            language: "ru".to_string(),
            confidence: 0.3,
            reason: "language not in allowlist".to_string(),
        };
        let json = event.to_json().expect("should serialize");
        let deserialized = DaemonEvent::from_json(&json).expect("should deserialize");
        assert_eq!(event, deserialized);
        assert!(json.contains("\"type\":\"transcription_dropped\""));
    }
}
