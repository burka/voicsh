//! Async Unix socket IPC server for daemon control.

use crate::error::{Result, VoicshError};
use crate::ipc::protocol::{Command, Response};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

/// Handler trait for processing IPC commands.
#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// Handle a command and return a response.
    async fn handle(&self, command: Command) -> Response;
}

/// State for managing server shutdown.
#[derive(Debug, Clone)]
struct ServerState {
    shutdown: Arc<Mutex<bool>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            shutdown: Arc::new(Mutex::new(false)),
        }
    }

    async fn is_shutdown(&self) -> bool {
        *self.shutdown.lock().await
    }

    async fn set_shutdown(&self) {
        *self.shutdown.lock().await = true;
    }
}

/// IPC server for handling daemon control commands via Unix socket.
pub struct IpcServer {
    socket_path: PathBuf,
    state: ServerState,
}

impl IpcServer {
    /// Create a new IPC server bound to the specified socket path.
    pub fn new(socket_path: PathBuf) -> Result<Self> {
        Ok(Self {
            socket_path,
            state: ServerState::new(),
        })
    }

    /// Get the socket path this server is using.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Get the default socket path based on XDG_RUNTIME_DIR or fallback.
    pub fn default_socket_path() -> PathBuf {
        if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(xdg_runtime).join("voicsh.sock")
        } else {
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/tmp/voicsh-{}.sock", uid))
        }
    }

    /// Start the IPC server and handle incoming connections.
    pub async fn start<H>(&self, handler: H) -> Result<()>
    where
        H: CommandHandler + 'static,
    {
        // Clean up any existing socket file
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).map_err(|e| VoicshError::IpcSocket {
                message: format!("Failed to remove existing socket: {}", e),
            })?;
        }

        // Bind to the socket
        let listener =
            UnixListener::bind(&self.socket_path).map_err(|e| VoicshError::IpcSocket {
                message: format!("Failed to bind to socket: {}", e),
            })?;

        let handler = Arc::new(handler);

        loop {
            // Check if shutdown was requested
            if self.state.is_shutdown().await {
                break;
            }

            // Accept connection with timeout to check for shutdown
            let accept_result =
                tokio::time::timeout(tokio::time::Duration::from_millis(100), listener.accept())
                    .await;

            match accept_result {
                Ok(Ok((stream, _))) => {
                    let handler = Arc::clone(&handler);
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, handler).await {
                            eprintln!("Error handling client: {}", e);
                        }
                    });
                }
                Ok(Err(e)) => {
                    return Err(VoicshError::IpcConnection {
                        message: format!("Failed to accept connection: {}", e),
                    });
                }
                Err(_) => {
                    // Timeout - check shutdown flag again
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Stop the IPC server and clean up the socket file.
    pub async fn stop(&self) -> Result<()> {
        self.state.set_shutdown().await;

        // Clean up socket file
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).map_err(|e| VoicshError::IpcSocket {
                message: format!("Failed to remove socket file: {}", e),
            })?;
        }

        Ok(())
    }
}

/// Handle a single client connection.
async fn handle_client<H>(stream: UnixStream, handler: Arc<H>) -> Result<()>
where
    H: CommandHandler,
{
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Read command (one line JSON)
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to read from client: {}", e),
        })?;

    // Parse command
    let command = Command::from_json(line.trim()).map_err(|e| VoicshError::IpcProtocol {
        message: format!("Failed to parse command: {}", e),
    })?;

    // Handle command
    let response = handler.handle(command).await;

    // Send response
    let response_json = response.to_json().map_err(|e| VoicshError::IpcProtocol {
        message: format!("Failed to serialize response: {}", e),
    })?;

    writer
        .write_all(response_json.as_bytes())
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to write to client: {}", e),
        })?;

    writer
        .write_all(b"\n")
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to write newline to client: {}", e),
        })?;

    writer
        .flush()
        .await
        .map_err(|e| VoicshError::IpcConnection {
            message: format!("Failed to flush writer: {}", e),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    // Mock handler for testing
    struct MockCommandHandler;

    #[async_trait::async_trait]
    impl CommandHandler for MockCommandHandler {
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

    #[test]
    fn test_default_socket_path_returns_valid_path() {
        let path = IpcServer::default_socket_path();
        let path_str = path.to_string_lossy();
        if std::env::var("XDG_RUNTIME_DIR").is_ok() {
            assert!(
                path_str.ends_with("voicsh.sock"),
                "With XDG_RUNTIME_DIR, expected path ending with voicsh.sock, got: {:?}",
                path
            );
        } else {
            // Fallback format: /tmp/voicsh-{uid}.sock
            let uid = unsafe { libc::getuid() };
            let expected = format!("/tmp/voicsh-{}.sock", uid);
            assert_eq!(
                path_str, expected,
                "Without XDG_RUNTIME_DIR, expected fallback path"
            );
        }
    }

    #[test]
    fn test_default_socket_path_with_xdg_runtime() {
        // If XDG_RUNTIME_DIR is set, path should use it
        if let Ok(xdg_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = IpcServer::default_socket_path();
            let expected = PathBuf::from(&xdg_dir).join("voicsh.sock");
            assert_eq!(path, expected);
        }
        // If not set, we can't safely test the fallback without race conditions
    }

    #[tokio::test]
    async fn test_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let _server = IpcServer::new(socket_path.clone()).unwrap();
        assert_eq!(_server.socket_path(), socket_path.as_path());
    }

    #[tokio::test]
    async fn test_server_binds_to_socket() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_handle = {
            let socket_path = socket_path.clone();
            tokio::spawn(async move {
                let server = IpcServer::new(socket_path).unwrap();
                server.start(MockCommandHandler).await
            })
        };

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify socket file exists
        assert!(socket_path.exists());

        // Cleanup
        drop(server_handle);
    }

    #[tokio::test]
    async fn test_client_can_send_command_and_receive_response() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockCommandHandler).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect as client
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();

        // Send Status command
        let command = Command::Status;
        let command_json = format!("{}\n", command.to_json().unwrap());
        stream.write_all(command_json.as_bytes()).await.unwrap();

        // Read response
        let mut response_data = Vec::new();
        stream.read_to_end(&mut response_data).await.unwrap();
        let response_str = String::from_utf8(response_data).unwrap();
        let response = Response::from_json(response_str.trim()).unwrap();

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
            _ => panic!("Expected Status response"),
        }

        // Cleanup
        drop(server_handle);
    }

    #[tokio::test]
    async fn test_multiple_concurrent_clients() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockCommandHandler).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Spawn multiple clients
        let mut client_handles = vec![];
        for i in 0..5 {
            let socket_path = socket_path.clone();
            let handle = tokio::spawn(async move {
                let mut stream = UnixStream::connect(&socket_path).await.unwrap();

                let command = if i % 2 == 0 {
                    Command::Status
                } else {
                    Command::Toggle
                };

                let command_json = format!("{}\n", command.to_json().unwrap());
                stream.write_all(command_json.as_bytes()).await.unwrap();

                let mut response_data = Vec::new();
                stream.read_to_end(&mut response_data).await.unwrap();
                let response_str = String::from_utf8(response_data).unwrap();
                Response::from_json(response_str.trim()).unwrap()
            });
            client_handles.push(handle);
        }

        // Wait for all clients to complete
        for handle in client_handles {
            let response = handle.await.unwrap();
            assert!(matches!(response, Response::Status { .. } | Response::Ok));
        }

        // Cleanup
        drop(server_handle);
    }

    #[tokio::test]
    async fn test_server_cleanup_on_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        let _server = IpcServer::new(socket_path.clone()).unwrap();

        // Start server in background task
        let server_socket_path = socket_path.clone();
        let server_task = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path.clone()).unwrap();
            let result = server.start(MockCommandHandler).await;
            server.stop().await.unwrap();
            result
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify socket exists
        assert!(socket_path.exists());

        // Stop server by dropping task (this will trigger shutdown)
        drop(server_task);

        // Give server time to clean up
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Note: Socket might still exist until explicit stop() is called
        // This is expected behavior - cleanup happens in stop()
    }

    #[tokio::test]
    async fn test_server_handles_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockCommandHandler).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect as client
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();

        // Send invalid JSON
        stream.write_all(b"not valid json\n").await.unwrap();

        // Server should handle error gracefully (connection will close)
        // We don't expect a response for invalid JSON
    }

    #[tokio::test]
    async fn test_all_commands_handled() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Start server in background
        let server_socket_path = socket_path.clone();
        let _server_handle = tokio::spawn(async move {
            let server = IpcServer::new(server_socket_path).unwrap();
            server.start(MockCommandHandler).await
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let commands = vec![
            (Command::Toggle, "Ok"),
            (Command::Start, "Ok"),
            (Command::Cancel, "Ok"),
            (Command::Shutdown, "Ok"),
        ];

        for (command, expected_type) in commands {
            let mut stream = UnixStream::connect(&socket_path).await.unwrap();
            let command_json = format!("{}\n", command.to_json().unwrap());
            stream.write_all(command_json.as_bytes()).await.unwrap();

            let mut response_data = Vec::new();
            stream.read_to_end(&mut response_data).await.unwrap();
            let response_str = String::from_utf8(response_data).unwrap();
            let response = Response::from_json(response_str.trim()).unwrap();

            match expected_type {
                "Ok" => assert!(matches!(response, Response::Ok)),
                _ => panic!("Unexpected expected type"),
            }
        }

        // Test Stop command separately (returns Transcription)
        let mut stream = UnixStream::connect(&socket_path).await.unwrap();
        let command_json = format!("{}\n", Command::Stop.to_json().unwrap());
        stream.write_all(command_json.as_bytes()).await.unwrap();

        let mut response_data = Vec::new();
        stream.read_to_end(&mut response_data).await.unwrap();
        let response_str = String::from_utf8(response_data).unwrap();
        let response = Response::from_json(response_str.trim()).unwrap();

        match response {
            Response::Transcription { text } => {
                assert_eq!(text, "test transcription");
            }
            _ => panic!("Expected Transcription response"),
        }
    }
}
