//! Text injection system for Wayland with testable command execution.
//!
//! Provides two injection mechanisms:
//! - Clipboard-based: Uses wl-copy and ydotool to paste via clipboard
//! - Direct typing: Uses ydotool to simulate keyboard input
//!
//! The `CommandExecutor` trait enables full testability without external dependencies.

use crate::error::{Result, VoicshError};
use std::process::{Command, Stdio};

/// Trait for executing system commands.
///
/// Object-safe, Send + Sync for use in concurrent contexts.
/// Enables testability by allowing mock implementations.
pub trait CommandExecutor: Send + Sync {
    /// Execute a command with arguments.
    ///
    /// Returns Ok(()) on success.
    /// Returns an error if the command fails or is not found.
    fn execute(&self, command: &str, args: &[&str]) -> Result<()>;
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
    fn execute(&self, command: &str, args: &[&str]) -> Result<()> {
        // Use status() instead of output() to avoid pipe creation.
        // Programs like wl-copy detect pipes and stay in foreground,
        // causing wait() to block forever. status() inherits stdio,
        // allowing them to fork to daemon immediately.
        let status = Command::new(command)
            .args(args)
            .stderr(Stdio::null())
            .status()
            .map_err(|e| {
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

        if !status.success() {
            return Err(VoicshError::InjectionFailed {
                message: format!("{} failed with status {:?}", command, status),
            });
        }

        Ok(())
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
    /// Uses wl-copy to set clipboard content, then simulates the given paste key.
    /// Tries wtype first (no daemon needed), falls back to ydotool.
    ///
    /// The `paste_key` argument controls which key combo is simulated
    /// (e.g. `"ctrl+v"` for GUI apps, `"ctrl+shift+v"` for terminals).
    ///
    /// # Requirements
    /// - wl-copy (from wl-clipboard package)
    /// - wtype (preferred) or ydotool (with ydotoold daemon)
    ///
    /// # Installation
    /// Ubuntu/Debian: `sudo apt install wl-clipboard wtype`
    /// Arch: `sudo pacman -S wl-clipboard wtype`
    pub fn inject_via_clipboard(&self, text: &str, paste_key: &str) -> Result<()> {
        use crate::input::focused_window::paste_key_to_wtype_args;

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

        // Delay to ensure clipboard is updated before paste
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Build wtype args from paste_key
        let wtype_args = paste_key_to_wtype_args(paste_key);
        let wtype_arg_refs: Vec<&str> = wtype_args.iter().map(String::as_str).collect();

        // Try wtype first (simpler, no daemon needed)
        if self.executor.execute("wtype", &wtype_arg_refs).is_ok() {
            return Ok(());
        }

        // Fall back to ydotool (small delay for reliability)
        self.executor
            .execute("ydotool", &["key", "--delay", "10", paste_key])
            .map_err(|e| match &e {
                VoicshError::InjectionToolNotFound { tool } if tool == "ydotool" => {
                    VoicshError::InjectionFailed {
                        message: "Neither wtype nor ydotool available for paste simulation.\n\
                            Install wtype (recommended, no daemon needed):\n\
                            Ubuntu/Debian: sudo apt install wtype\n\
                            Arch: sudo pacman -S wtype"
                            .to_string(),
                    }
                }
                _ => e,
            })?;

        Ok(())
    }

    /// Inject text directly by simulating keyboard input.
    ///
    /// Tries wtype first (no daemon needed), falls back to ydotool.
    ///
    /// # Requirements
    /// - wtype (preferred) or ydotool (with ydotoold daemon)
    ///
    /// # Installation
    /// Ubuntu/Debian: `sudo apt install wtype`
    /// Arch: `sudo pacman -S wtype`
    pub fn inject_direct(&self, text: &str) -> Result<()> {
        // Try wtype first (simpler, no daemon needed)
        if self.executor.execute("wtype", &[text]).is_ok() {
            return Ok(());
        }

        // Fall back to ydotool (small delay for reliability)
        self.executor
            .execute("ydotool", &["type", "--delay", "10", text])
            .map_err(|e| match &e {
                VoicshError::InjectionToolNotFound { tool } if tool == "ydotool" => {
                    VoicshError::InjectionFailed {
                        message: "Neither wtype nor ydotool available for text injection.\n\
                            Install wtype (recommended, no daemon needed):\n\
                            Ubuntu/Debian: sudo apt install wtype\n\
                            Arch: sudo pacman -S wtype"
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
        responses: Mutex<VecDeque<Result<()>>>,
    }

    impl MockCommandExecutor {
        pub fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::new()),
            }
        }

        /// Add a successful response to the queue.
        pub fn with_success(self) -> Self {
            self.responses.lock().unwrap().push_back(Ok(()));
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
        fn execute(&self, command: &str, args: &[&str]) -> Result<()> {
            // Record the call
            self.calls.lock().unwrap().push((
                command.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));

            // Return the next configured response or a default success
            self.responses.lock().unwrap().pop_front().unwrap_or(Ok(()))
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
        fn execute(&self, command: &str, args: &[&str]) -> Result<()> {
            self.calls.lock().unwrap().push((
                command.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            Ok(())
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
        mock.execute("ydotool", &["key", "ctrl+v"]).unwrap();

        assert_eq!(mock.call_count(), 2);

        let call1 = mock.call(0).unwrap();
        assert_eq!(call1.0, "wl-copy");
        assert_eq!(call1.1, vec!["hello"]);

        let call2 = mock.call(1).unwrap();
        assert_eq!(call2.0, "ydotool");
        assert_eq!(call2.1, vec!["key", "ctrl+v"]);
    }

    #[test]
    fn test_mock_executor_returns_configured_response() {
        let mock = MockCommandExecutor::new().with_success().with_success();

        let result1 = mock.execute("cmd1", &[]);
        assert!(result1.is_ok());

        let result2 = mock.execute("cmd2", &[]);
        assert!(result2.is_ok());

        // After configured responses are exhausted, returns success by default
        let result3 = mock.execute("cmd3", &[]);
        assert!(result3.is_ok());
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

        injector
            .inject_via_clipboard("Hello, World!", "ctrl+v")
            .unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 2);

        // First call: wl-copy with the text
        assert_eq!(calls[0].0, "wl-copy");
        assert_eq!(calls[0].1, vec!["Hello, World!"]);

        // Second call: wtype to simulate Ctrl+V (preferred over ydotool)
        assert_eq!(calls[1].0, "wtype");
        assert_eq!(calls[1].1, vec!["-M", "ctrl", "-k", "v"]);
    }

    #[test]
    fn test_inject_via_clipboard_terminal_paste_key() {
        let mock = MockCommandExecutor::new();
        let injector = TextInjector::new(mock);

        injector
            .inject_via_clipboard("Hello", "ctrl+shift+v")
            .unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 2);

        assert_eq!(calls[1].0, "wtype");
        assert_eq!(calls[1].1, vec!["-M", "ctrl", "-M", "shift", "-k", "v"]);
    }

    #[test]
    fn test_inject_via_clipboard_handles_wl_copy_error() {
        let mock = MockCommandExecutor::new().with_error(VoicshError::InjectionToolNotFound {
            tool: "wl-copy".to_string(),
        });
        let injector = TextInjector::new(mock);

        let result = injector.inject_via_clipboard("test", "ctrl+v");
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
    fn test_inject_via_clipboard_falls_back_to_ydotool() {
        // wtype fails, should fall back to ydotool
        let mock = MockCommandExecutor::new()
            .with_success() // wl-copy succeeds
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "wtype".to_string(),
            }) // wtype fails
            .with_success(); // ydotool succeeds
        let injector = TextInjector::new(mock);

        let result = injector.inject_via_clipboard("test", "ctrl+v");
        assert!(result.is_ok());

        // Should have called wl-copy, then wtype (failed), then ydotool
        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "wl-copy");
        assert_eq!(calls[1].0, "wtype");
        assert_eq!(calls[2].0, "ydotool");
        assert_eq!(calls[2].1, vec!["key", "--delay", "10", "ctrl+v"]);
    }

    #[test]
    fn test_inject_via_clipboard_ydotool_terminal_paste_key() {
        // wtype fails, ydotool should receive ctrl+shift+v
        let mock = MockCommandExecutor::new()
            .with_success() // wl-copy succeeds
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "wtype".to_string(),
            }) // wtype fails
            .with_success(); // ydotool succeeds
        let injector = TextInjector::new(mock);

        injector
            .inject_via_clipboard("test", "ctrl+shift+v")
            .unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls[2].0, "ydotool");
        assert_eq!(calls[2].1, vec!["key", "--delay", "10", "ctrl+shift+v"]);
    }

    #[test]
    fn test_inject_direct_calls_correct_commands() {
        let mock = MockCommandExecutor::new();
        let injector = TextInjector::new(mock);

        injector.inject_direct("Hello").unwrap();

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 1);

        // wtype is preferred over ydotool
        assert_eq!(calls[0].0, "wtype");
        assert_eq!(calls[0].1, vec!["Hello"]);
    }

    #[test]
    fn test_inject_direct_falls_back_to_ydotool() {
        // wtype fails, falls back to ydotool
        let mock = MockCommandExecutor::new()
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "wtype".to_string(),
            })
            .with_success(); // ydotool succeeds
        let injector = TextInjector::new(mock);

        let result = injector.inject_direct("test");
        assert!(result.is_ok());

        let calls = injector.executor.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "wtype");
        assert_eq!(calls[1].0, "ydotool");
        assert_eq!(calls[1].1, vec!["type", "--delay", "10", "test"]);
    }

    #[test]
    fn test_inject_direct_fails_when_both_unavailable() {
        // Both wtype and ydotool fail
        let mock = MockCommandExecutor::new()
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "wtype".to_string(),
            })
            .with_error(VoicshError::InjectionToolNotFound {
                tool: "ydotool".to_string(),
            });
        let injector = TextInjector::new(mock);

        let result = injector.inject_direct("test");
        assert!(result.is_err());

        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                assert!(message.contains("wtype"));
            }
            _ => panic!("Expected InjectionFailed error"),
        }
    }

    #[test]
    fn test_inject_via_clipboard_with_special_characters() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        let text_with_special = "Hello\nWorld\t!@#$%";
        injector
            .inject_via_clipboard(text_with_special, "ctrl+v")
            .unwrap();

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
        // wtype is tried first
        assert_eq!(calls[0].0, "wtype");
        assert_eq!(calls[0].1, vec![unicode_text]);
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
            .with_success()
            .with_error(VoicshError::InjectionFailed {
                message: "error".to_string(),
            })
            .with_success();

        assert!(mock.execute("cmd1", &[]).is_ok());
        assert!(mock.execute("cmd2", &[]).is_err());
        assert!(mock.execute("cmd3", &[]).is_ok());
    }

    #[test]
    fn test_inject_via_clipboard_empty_string() {
        let recorder = RecordingExecutor::new();
        let injector = TextInjector::new(recorder);

        injector.inject_via_clipboard("", "ctrl+v").unwrap();

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
        // wtype is tried first
        assert_eq!(calls[0].0, "wtype");
        assert_eq!(calls[0].1, vec![""]);
    }
}
