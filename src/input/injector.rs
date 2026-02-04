//! Text injection system for Wayland with testable command execution.
//!
//! Provides two injection mechanisms:
//! - Clipboard-based: Uses wl-copy and ydotool to paste via clipboard
//! - Direct typing: Uses ydotool to simulate keyboard input
//!
//! The `CommandExecutor` trait enables full testability without external dependencies.

use crate::error::{Result, VoicshError};
use std::process::Command;

/// Trait for executing system commands.
///
/// Object-safe, Send + Sync for use in concurrent contexts.
/// Enables testability by allowing mock implementations.
pub trait CommandExecutor: Send + Sync {
    /// Execute a command with arguments.
    ///
    /// Returns the stdout of the command on success.
    /// Returns an error if the command fails or is not found.
    fn execute(&self, command: &str, args: &[&str]) -> Result<String>;
}

/// Production command executor using std::process::Command.
#[derive(Debug, Clone, Default)]
pub struct SystemCommandExecutor;

impl SystemCommandExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, command: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(command).args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VoicshError::InjectionToolNotFound {
                    tool: command.to_string(),
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                VoicshError::InjectionPermissionDenied {
                    message: format!(
                        "Permission denied executing {}: {}.\n\
                        Hint: If using ydotool, ensure the ydotoold daemon is running and you have permissions.\n\
                        Try: sudo systemctl start ydotool",
                        command, e
                    ),
                }
            } else {
                VoicshError::InjectionFailed {
                    message: format!("Failed to execute {}: {}", command, e),
                }
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoicshError::InjectionFailed {
                message: format!(
                    "{} failed with status {:?}: {}",
                    command, output.status, stderr
                ),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Text injector that uses CommandExecutor for system interaction.
pub struct TextInjector<E: CommandExecutor> {
    executor: E,
}

impl<E: CommandExecutor> TextInjector<E> {
    /// Create a new TextInjector with the given executor.
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Inject text via clipboard mechanism.
    ///
    /// Uses wl-copy to set clipboard content, then ydotool to simulate Ctrl+V.
    /// This is more reliable for complex text with special characters.
    ///
    /// # Requirements
    /// - wl-copy (from wl-clipboard package)
    /// - ydotool (with ydotoold daemon running)
    ///
    /// # Installation
    /// Ubuntu/Debian: `sudo apt install wl-clipboard ydotool`
    /// Arch: `sudo pacman -S wl-clipboard ydotool`
    ///
    /// # Setup
    /// Ensure ydotoold daemon is running:
    /// `sudo systemctl enable --now ydotool`
    pub fn inject_via_clipboard(&self, text: &str) -> Result<()> {
        // Copy text to clipboard using wl-copy
        self.executor
            .execute("wl-copy", &[text])
            .map_err(|e| match &e {
                VoicshError::InjectionToolNotFound { tool } if tool == "wl-copy" => {
                    VoicshError::InjectionFailed {
                        message: "wl-copy not found. Install wl-clipboard:\n\
                            Ubuntu/Debian: sudo apt install wl-clipboard\n\
                            Arch: sudo pacman -S wl-clipboard"
                            .to_string(),
                    }
                }
                _ => e,
            })?;

        // Simulate Ctrl+V using ydotool
        // key 29:1 = left ctrl down
        // key 47:1 = v down
        // key 47:0 = v up
        // key 29:0 = left ctrl up
        self.executor
            .execute("ydotool", &["key", "29:1", "47:1", "47:0", "29:0"])
            .map_err(|e| match &e {
                VoicshError::InjectionToolNotFound { tool } if tool == "ydotool" => {
                    VoicshError::InjectionFailed {
                        message: "ydotool not found. Install ydotool and start the daemon:\n\
                            Ubuntu/Debian: sudo apt install ydotool\n\
                            Arch: sudo pacman -S ydotool\n\
                            Then start the daemon: sudo systemctl enable --now ydotool"
                            .to_string(),
                    }
                }
                _ => e,
            })?;

        Ok(())
    }

    /// Inject text directly by simulating keyboard input.
    ///
    /// Uses ydotool to type each character.
    /// May have issues with special characters or non-ASCII text.
    ///
    /// # Requirements
    /// - ydotool (with ydotoold daemon running)
    ///
    /// # Installation
    /// Ubuntu/Debian: `sudo apt install ydotool`
    /// Arch: `sudo pacman -S ydotool`
    ///
    /// # Setup
    /// Ensure ydotoold daemon is running:
    /// `sudo systemctl enable --now ydotool`
    pub fn inject_direct(&self, text: &str) -> Result<()> {
        self.executor
            .execute("ydotool", &["type", text])
            .map_err(|e| match &e {
                VoicshError::InjectionToolNotFound { tool } if tool == "ydotool" => {
                    VoicshError::InjectionFailed {
                        message: "ydotool not found. Install ydotool and start the daemon:\n\
                            Ubuntu/Debian: sudo apt install ydotool\n\
                            Arch: sudo pacman -S ydotool\n\
                            Then start the daemon: sudo systemctl enable --now ydotool"
                            .to_string(),
                    }
                }
                _ => e,
            })?;
        Ok(())
    }
}

impl TextInjector<SystemCommandExecutor> {
    /// Create a TextInjector with the system command executor.
    pub fn system() -> Self {
        Self::new(SystemCommandExecutor::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Mock command executor for testing.
    ///
    /// Records all command executions and returns configured responses.
    #[derive(Debug)]
    pub struct MockCommandExecutor {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        responses: Mutex<VecDeque<Result<String>>>,
    }

    impl MockCommandExecutor {
        pub fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::new()),
            }
        }

        /// Add a successful response to the queue.
        pub fn with_response(self, response: &str) -> Self {
            self.responses
                .lock()
                .unwrap()
                .push_back(Ok(response.to_string()));
            self
        }

        /// Add an error response to the queue.
        pub fn with_error(self, error: VoicshError) -> Self {
            self.responses.lock().unwrap().push_back(Err(error));
            self
        }

        /// Get all recorded calls.
        pub fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }

        /// Get the number of recorded calls.
        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        /// Get a specific call by index.
        pub fn call(&self, index: usize) -> Option<(String, Vec<String>)> {
            self.calls.lock().unwrap().get(index).cloned()
        }

        /// Clear all recorded calls.
        pub fn clear_calls(&self) {
            self.calls.lock().unwrap().clear();
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn execute(&self, command: &str, args: &[&str]) -> Result<String> {
            // Record the call
            self.calls.lock().unwrap().push((
                command.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));

            // Return the next configured response or a default success
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    /// Recording executor that captures calls but always succeeds.
    #[derive(Debug)]
    pub struct RecordingExecutor {
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl RecordingExecutor {
        pub fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        pub fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandExecutor for RecordingExecutor {
        fn execute(&self, command: &str, args: &[&str]) -> Result<String> {
            self.calls.lock().unwrap().push((
                command.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            Ok(String::new())
        }
    }

    #[test]
    fn test_command_executor_is_object_safe() {
        // This test verifies that CommandExecutor can be used as a trait object
        let executor: Box<dyn CommandExecutor> = Box::new(MockCommandExecutor::new());
        let result = executor.execute("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_executor_records_calls() {
        let mock = MockCommandExecutor::new();

        mock.execute("wl-copy", &["hello"]).unwrap();
        mock.execute("ydotool", &["key", "29:1"]).unwrap();

        assert_eq!(mock.call_count(), 2);

        let call1 = mock.call(0).unwrap();
        assert_eq!(call1.0, "wl-copy");
        assert_eq!(call1.1, vec!["hello"]);

        let call2 = mock.call(1).unwrap();
        assert_eq!(call2.0, "ydotool");
        assert_eq!(call2.1, vec!["key", "29:1"]);
    }

    #[test]
    fn test_mock_executor_returns_configured_response() {
        let mock = MockCommandExecutor::new()
            .with_response("output1")
            .with_response("output2");

        let result1 = mock.execute("cmd1", &[]).unwrap();
        assert_eq!(result1, "output1");

        let result2 = mock.execute("cmd2", &[]).unwrap();
        assert_eq!(result2, "output2");

        // After configured responses are exhausted, returns empty string
        let result3 = mock.execute("cmd3", &[]).unwrap();
        assert_eq!(result3, "");
    }

    #[test]
    fn test_mock_executor_returns_configured_error() {
        let mock = MockCommandExecutor::new().with_error(VoicshError::InjectionToolNotFound {
            tool: "missing-tool".to_string(),
        });

        let result = mock.execute("missing-tool", &[]);
        assert!(result.is_err());

        match result {
            Err(VoicshError::InjectionToolNotFound { tool }) => {
                assert_eq!(tool, "missing-tool");
            }
            _ => panic!("Expected InjectionToolNotFound error"),
        }
    }

    #[test]
    fn test_mock_executor_clear_calls() {
        let mock = MockCommandExecutor::new();

        mock.execute("cmd", &[]).unwrap();
        assert_eq!(mock.call_count(), 1);

        mock.clear_calls();
        assert_eq!(mock.call_count(), 0);
    }

    #[test]
    fn test_recording_executor_captures_calls() {
        let recorder = RecordingExecutor::new();

        recorder.execute("cmd1", &["arg1", "arg2"]).unwrap();
        recorder.execute("cmd2", &[]).unwrap();

        let calls = recorder.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "cmd1");
        assert_eq!(calls[0].1, vec!["arg1", "arg2"]);
        assert_eq!(calls[1].0, "cmd2");
        assert!(calls[1].1.is_empty());
    }

    #[test]
    fn test_inject_via_clipboard_calls_correct_commands() {
        let mock = MockCommandExecutor::new();
        let injector = TextInjector::new(mock);

        injector.inject_via_clipboard("Hello, World!").unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 2);

        // First call: wl-copy with the text
        assert_eq!(calls[0].0, "wl-copy");
        assert_eq!(calls[0].1, vec!["Hello, World!"]);

        // Second call: ydotool to simulate Ctrl+V
        assert_eq!(calls[1].0, "ydotool");
        assert_eq!(calls[1].1, vec!["key", "29:1", "47:1", "47:0", "29:0"]);
    }

    #[test]
    fn test_inject_via_clipboard_handles_wl_copy_error() {
        let mock = MockCommandExecutor::new().with_error(VoicshError::InjectionToolNotFound {
            tool: "wl-copy".to_string(),
        });
        let injector = TextInjector::new(mock);

        let result = injector.inject_via_clipboard("test");
        assert!(result.is_err());

        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                assert!(message.contains("wl-copy"));
                assert!(message.contains("wl-clipboard"));
            }
            _ => panic!("Expected InjectionFailed error with wl-copy installation instructions"),
        }
    }

    #[test]
    fn test_inject_via_clipboard_handles_ydotool_error() {
        let mock = MockCommandExecutor::new()
            .with_response("") // wl-copy succeeds
            .with_error(VoicshError::InjectionPermissionDenied {
                message: "ydotool requires permissions".to_string(),
            });
        let injector = TextInjector::new(mock);

        let result = injector.inject_via_clipboard("test");
        assert!(result.is_err());

        match result {
            Err(VoicshError::InjectionPermissionDenied { message }) => {
                assert!(message.contains("ydotool"));
            }
            _ => panic!("Expected InjectionPermissionDenied error"),
        }
    }

    #[test]
    fn test_inject_direct_calls_correct_commands() {
        let mock = MockCommandExecutor::new();
        let injector = TextInjector::new(mock);

        injector.inject_direct("Hello").unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 1);

        assert_eq!(calls[0].0, "ydotool");
        assert_eq!(calls[0].1, vec!["type", "Hello"]);
    }

    #[test]
    fn test_inject_direct_handles_error() {
        let mock = MockCommandExecutor::new().with_error(VoicshError::InjectionFailed {
            message: "ydotool type failed".to_string(),
        });
        let injector = TextInjector::new(mock);

        let result = injector.inject_direct("test");
        assert!(result.is_err());

        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                assert!(message.contains("ydotool"));
            }
            _ => panic!("Expected InjectionFailed error"),
        }
    }

    #[test]
    fn test_inject_via_clipboard_with_special_characters() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        let text_with_special = "Hello\nWorld\t!@#$%";
        injector.inject_via_clipboard(text_with_special).unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls[0].1, vec![text_with_special]);
    }

    #[test]
    fn test_inject_direct_with_unicode() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        let unicode_text = "Hello 世界 🌍";
        injector.inject_direct(unicode_text).unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls[0].1, vec!["type", unicode_text]);
    }

    #[test]
    fn test_text_injector_system_constructor() {
        let _injector = TextInjector::system();
        // Just verify it compiles and constructs
    }

    #[test]
    fn test_system_command_executor_new() {
        let _executor = SystemCommandExecutor::new();
        // Just verify it compiles and constructs
    }

    #[test]
    fn test_command_executor_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Box<dyn CommandExecutor>>();
    }

    #[test]
    fn test_command_executor_is_sync() {
        fn assert_sync<T: Sync>() {}
        assert_sync::<Box<dyn CommandExecutor>>();
    }

    #[test]
    fn test_mock_executor_with_multiple_errors() {
        let mock = MockCommandExecutor::new()
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "tool1".to_string(),
            })
            .with_error(VoicshError::InjectionPermissionDenied {
                message: "denied".to_string(),
            });

        let result1 = mock.execute("cmd1", &[]);
        assert!(matches!(
            result1,
            Err(VoicshError::InjectionToolNotFound { .. })
        ));

        let result2 = mock.execute("cmd2", &[]);
        assert!(matches!(
            result2,
            Err(VoicshError::InjectionPermissionDenied { .. })
        ));
    }

    #[test]
    fn test_mock_executor_builder_pattern() {
        let mock = MockCommandExecutor::new()
            .with_response("first")
            .with_error(VoicshError::InjectionFailed {
                message: "error".to_string(),
            })
            .with_response("second");

        assert_eq!(mock.execute("cmd1", &[]).unwrap(), "first");
        assert!(mock.execute("cmd2", &[]).is_err());
        assert_eq!(mock.execute("cmd3", &[]).unwrap(), "second");
    }

    #[test]
    fn test_inject_via_clipboard_empty_string() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        injector.inject_via_clipboard("").unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec![""]);
    }

    #[test]
    fn test_inject_direct_empty_string() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        injector.inject_direct("").unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, vec!["type", ""]);
    }
}
