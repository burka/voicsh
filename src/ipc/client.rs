//! IPC client for sending commands to the daemon.

use crate::error::{Result, VoicshError};
use crate::ipc::protocol::{Command, Response};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Send a command to the daemon via Unix socket.
///
/// # Arguments
/// * `socket_path` - Path to the Unix socket
/// * `command` - Command to send
///
/// # Returns
/// Response from daemon or error
///
/// # Errors
/// Returns `VoicshError::IpcConnection` if connection fails
/// Returns `VoicshError::IpcProtocol` if serialization/deserialization fails
pub async fn send_command(socket_path: &Path, command: Command) -> Result<Response> {
    // Connect to daemon socket
    let stream =
        UnixStream::connect(socket_path)
            .await
            .map_err(|e| VoicshError::IpcConnection {
                message: format!("Failed to connect to daemon: {}", e),
            })?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Serialize and send command
    let command_json = command.to_json().map_err(|e| VoicshError::IpcProtocol {
        message: format!("Failed to serialize command: {}", e),
    })?;

    writer
        .write_all(command_json.as_bytes())
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to write command: {}", e),
        })?;

    writer
        .write_all(b"\n")
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to write newline: {}", e),
        })?;

    writer
        .flush()
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to flush writer: {}", e),
        })?;

    // Read response
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to read response: {}", e),
        })?;

    // Deserialize response
    let response =
        Response::from_json(response_line.trim()).map_err(|e| VoicshError::IpcProtocol {
            message: format!("Failed to deserialize response: {}", e),
        })?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::server::{CommandHandler, IpcServer};
    use tempfile::TempDir;

    // Mock handler for testing
    struct MockHandler;

    #[async_trait::async_trait]
    impl CommandHandler for MockHandler {
        async fn handle(&self, command: Command) -> Response {
            match command {
                Command::Status => Response::Status {
                    recording: false,
                    model_loaded: true,
                    model_name: Some("test-model".to_string()),
                },
                Command::Toggle => Response::Ok,
                Command::Start => Response::Ok,
                Command::Stop => Response::Transcription {
                    text: "test transcription".to_string(),
                },
                Command::Cancel => Response::Ok,
                Command::Shutdown => Response::Ok,
            }
        }
    }

    #[tokio::test]
    async fn test_send_command_status() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send command via client
        let response = send_command(&socket_path, Command::Status).await.unwrap();

        // Verify response
        match response {
            Response::Status {
                recording,
                model_loaded,
                model_name,
            } => {
                assert!(!recording);
                assert!(model_loaded);
                assert_eq!(model_name, Some("test-model".to_string()));
            }
            _ => panic!("Expected Status response, got: {:?}", response),
        }
    }

    #[tokio::test]
    async fn test_send_command_toggle() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let response = send_command(&socket_path, Command::Toggle).await.unwrap();
        assert_eq!(response, Response::Ok);
    }

    #[tokio::test]
    async fn test_send_command_start() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let response = send_command(&socket_path, Command::Start).await.unwrap();
        assert_eq!(response, Response::Ok);
    }

    #[tokio::test]
    async fn test_send_command_stop() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let response = send_command(&socket_path, Command::Stop).await.unwrap();
        match response {
            Response::Transcription { text } => {
                assert_eq!(text, "test transcription");
            }
            _ => panic!("Expected Transcription response"),
        }
    }

    #[tokio::test]
    async fn test_send_command_cancel() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let response = send_command(&socket_path, Command::Cancel).await.unwrap();
        assert_eq!(response, Response::Ok);
    }

    #[tokio::test]
    async fn test_send_command_connection_failed() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("nonexistent.sock");

        // Try to connect to non-existent socket
        let result = send_command(&socket_path, Command::Status).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            VoicshError::IpcConnection { message } => {
                assert!(message.contains("Failed to connect to daemon"));
            }
            _ => panic!("Expected IpcConnection error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_multiple_sequential_commands() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send multiple commands
        let commands = vec![
            Command::Status,
            Command::Toggle,
            Command::Start,
            Command::Cancel,
        ];

        for cmd in commands {
            let response = send_command(&socket_path, cmd.clone()).await.unwrap();
            assert!(
                matches!(response, Response::Ok | Response::Status { .. }),
                "Unexpected response for {:?}: {:?}",
                cmd,
                response
            );
        }
    }

    #[tokio::test]
    async fn test_send_command_all_variants() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Test all command variants
        let _ = send_command(&socket_path, Command::Status).await.unwrap();
        let _ = send_command(&socket_path, Command::Toggle).await.unwrap();
        let _ = send_command(&socket_path, Command::Start).await.unwrap();
        let _ = send_command(&socket_path, Command::Stop).await.unwrap();
        let _ = send_command(&socket_path, Command::Cancel).await.unwrap();
        let _ = send_command(&socket_path, Command::Shutdown).await.unwrap();
    }
}
