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

/// Responses sent by daemon to CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Command succeeded
    Ok,
    /// Command succeeded with transcription result
    Transcription { text: String },
    /// Current daemon status
    Status {
        recording: bool,
        model_loaded: bool,
        model_name: Option<String>,
    },
    /// Error occurred
    Error { message: String },
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
        let resp = Response::Ok;
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
        let ok = Response::Ok.to_json().unwrap();
        assert_eq!(ok, r#"{"type":"ok"}"#);

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
}
