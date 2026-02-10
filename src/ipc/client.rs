//! IPC client for sending commands to the daemon.

use crate::error::{Result, VoicshError};
use crate::ipc::protocol::{Command, Response};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Timeout for IPC operations (5 seconds)
const IPC_TIMEOUT_SECS: u64 = 5;

/// Send a command to the daemon via Unix socket with timeout.
///
/// # Arguments
/// * `socket_path` - Path to the Unix socket
/// * `command` - Command to send
///
/// # Returns
/// Response from daemon or error
///
/// # Errors
/// Returns `VoicshError::IpcConnection` if connection fails or times out
/// Returns `VoicshError::IpcProtocol` if serialization/deserialization fails
pub async fn send_command(socket_path: &Path, command: Command) -> Result<Response> {
    let timeout = tokio::time::Duration::from_secs(IPC_TIMEOUT_SECS);

    tokio::time::timeout(timeout, send_command_inner(socket_path, command))
        .await
        .map_err(|_| VoicshError::IpcConnection {
            message: format!("Command timed out after {} seconds", IPC_TIMEOUT_SECS),
        })?
}

/// Helper to convert IO errors to IPC connection errors with context.
fn ipc_io_error(context: &str, e: std::io::Error) -> VoicshError {
    VoicshError::IpcConnection {
        message: format!("{}: {}", context, e),
    }
}

/// Internal implementation of send_command without timeout wrapper.
async fn send_command_inner(socket_path: &Path, command: Command) -> Result<Response> {
    // Connect to daemon socket
    let stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| ipc_io_error("Failed to connect to daemon", e))?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Serialize and send command
    let command_json = command.to_json().map_err(|e| VoicshError::IpcProtocol {
        message: format!("Failed to serialize command: {}", e),
    })?;

    writer
        .write_all(command_json.as_bytes())
        .await
        .map_err(|e| ipc_io_error("Failed to write command", e))?;

    writer
        .write_all(b"\n")
        .await
        .map_err(|e| ipc_io_error("Failed to write newline", e))?;

    writer
        .flush()
        .await
        .map_err(|e| ipc_io_error("Failed to flush writer", e))?;

    // Read response
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .await
        .map_err(|e| ipc_io_error("Failed to read response", e))?;

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
                    language: Some("auto".to_string()),
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
                language,
            } => {
                assert!(!recording);
                assert!(model_loaded);
                assert_eq!(model_name, Some("test-model".to_string()));
                assert_eq!(language, Some("auto".to_string()));
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

        // Test Status command
        let response = send_command(&socket_path, Command::Status).await.unwrap();
        match response {
            Response::Status {
                recording,
                model_loaded,
                model_name,
                language,
            } => {
                assert!(!recording, "Should not be recording");
                assert!(model_loaded, "Model should be loaded");
                assert_eq!(
                    model_name,
                    Some("test-model".to_string()),
                    "Model name should match"
                );
                assert_eq!(language, Some("auto".to_string()), "Language should match");
            }
            _ => panic!("Expected Status response, got: {:?}", response),
        }

        // Test Toggle command
        let response = send_command(&socket_path, Command::Toggle).await.unwrap();
        assert_eq!(response, Response::Ok, "Toggle should return Ok");

        // Test Start command
        let response = send_command(&socket_path, Command::Start).await.unwrap();
        assert_eq!(response, Response::Ok, "Start should return Ok");

        // Test Stop command
        let response = send_command(&socket_path, Command::Stop).await.unwrap();
        match response {
            Response::Transcription { text } => {
                assert_eq!(
                    text, "test transcription",
                    "Transcription text should match"
                );
            }
            _ => panic!("Expected Transcription response, got: {:?}", response),
        }

        // Test Cancel command
        let response = send_command(&socket_path, Command::Cancel).await.unwrap();
        assert_eq!(response, Response::Ok, "Cancel should return Ok");

        // Test Shutdown command
        let response = send_command(&socket_path, Command::Shutdown).await.unwrap();
        assert_eq!(response, Response::Ok, "Shutdown should return Ok");
    }

    // Race condition tests (reproducible with deterministic timing)
    #[tokio::test]
    async fn test_concurrent_commands_with_seed() {
        use std::sync::atomic::{AtomicU64, Ordering};

        // Seed for reproducibility - change this to test different scenarios
        const SEED: u64 = 42;
        static CALL_COUNT: AtomicU64 = AtomicU64::new(0);

        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Launch concurrent commands with deterministic delays based on seed
        let mut handles = vec![];
        for i in 0..10 {
            let path = socket_path.clone();
            let delay_ms = (SEED + i * 7) % 20; // Deterministic delay based on seed

            let handle = tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                let response = send_command(&path, Command::Status).await.unwrap();
                CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                response
            });
            handles.push(handle);
        }

        // Wait for all commands to complete
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }

        // All commands should succeed
        assert_eq!(results.len(), 10, "All spawned tasks should complete");
        assert_eq!(
            CALL_COUNT.load(Ordering::SeqCst),
            10,
            "All 10 commands should have completed"
        );

        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "Task {} should not panic", i);
            let response = result.as_ref().unwrap();
            assert!(
                matches!(response, Response::Status { .. }),
                "All responses should be Status"
            );
        }
    }

    #[tokio::test]
    async fn test_interleaved_command_types_with_seed() {
        // Seed for reproducibility
        const SEED: u64 = 123;

        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Interleave different command types with deterministic ordering
        let commands = vec![
            Command::Status,
            Command::Toggle,
            Command::Start,
            Command::Status,
            Command::Stop,
            Command::Cancel,
        ];

        let mut handles = vec![];
        for (i, cmd) in commands.into_iter().enumerate() {
            let path = socket_path.clone();
            let delay_ms = (SEED + i as u64 * 13) % 30; // Deterministic delay

            let handle = tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                send_command(&path, cmd).await
            });
            handles.push(handle);
        }

        // Wait for all commands
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }

        // All should complete successfully
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "Task {} should not panic", i);
            assert!(
                result.as_ref().unwrap().is_ok(),
                "Command {} should succeed",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_rapid_fire_commands_with_seed() {
        // Test rapid commands with minimal delays
        const SEED: u64 = 999;

        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockHandler).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send 20 rapid commands with very small delays
        let mut handles = vec![];
        for i in 0..20 {
            let path = socket_path.clone();
            let delay_us = (SEED + i * 3) % 1000; // Microsecond delays

            let handle = tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_micros(delay_us)).await;
                send_command(&path, Command::Status).await
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }

        // All rapid commands should complete
        let success_count = results
            .iter()
            .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
            .count();

        assert_eq!(success_count, 20, "All 20 rapid commands should succeed");
    }
}
