//! Injector station that outputs transcribed text to the system.

use crate::config::InputMethod;
use crate::continuous::error::StationError;
use crate::continuous::station::Station;
use crate::continuous::types::TranscribedText;
use crate::input::injector::TextInjector;

/// Station that injects transcribed text into the system.
///
/// This is a terminal station that outputs text via either clipboard or direct injection.
/// It does not produce any output for downstream stations.
pub struct InjectorStation {
    injector: TextInjector<crate::input::injector::SystemCommandExecutor>,
    method: InputMethod,
    quiet: bool,
}

impl InjectorStation {
    /// Create a new injector station with the specified input method.
    pub fn new(method: InputMethod) -> Self {
        Self {
            injector: TextInjector::system(),
            method,
            quiet: false,
        }
    }

    /// Configure whether to suppress output to stderr.
    ///
    /// When quiet is false (default), the transcribed text is printed to stderr.
    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

impl Station for InjectorStation {
    type Input = TranscribedText;
    type Output = (); // Terminal station - doesn't produce output

    fn name(&self) -> &'static str {
        "injector"
    }

    fn process(&mut self, text: TranscribedText) -> Result<Option<()>, StationError> {
        // Print transcription to stderr if not quiet
        if !self.quiet {
            eprintln!("\"{}\"", text.text);
        }

        // Inject text via configured method
        let result = match self.method {
            InputMethod::Clipboard => self.injector.inject_via_clipboard(&text.text),
            InputMethod::Direct => self.injector.inject_direct(&text.text),
        };

        // Convert injection errors to recoverable station errors
        match result {
            Ok(()) => Ok(Some(())),
            Err(e) => Err(StationError::Recoverable(format!(
                "Injection failed: {}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::injector::{CommandExecutor, TextInjector};
    use std::sync::Mutex;
    use std::time::Instant;

    /// Mock command executor for testing.
    #[derive(Debug, Clone)]
    struct MockCommandExecutor {
        calls: std::sync::Arc<Mutex<Vec<(String, Vec<String>)>>>,
        should_fail: bool,
    }

    impl MockCommandExecutor {
        fn new() -> Self {
            Self {
                calls: std::sync::Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }

        fn with_failure() -> Self {
            Self {
                calls: std::sync::Arc::new(Mutex::new(Vec::new())),
                should_fail: true,
            }
        }

        fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn execute(&self, command: &str, args: &[&str]) -> crate::error::Result<()> {
            self.calls.lock().unwrap().push((
                command.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));

            if self.should_fail {
                Err(crate::error::VoicshError::InjectionFailed {
                    message: "Mock failure".to_string(),
                })
            } else {
                Ok(())
            }
        }
    }

    /// Mock injector station for testing with mock executor.
    struct MockInjectorStation {
        injector: TextInjector<MockCommandExecutor>,
        executor_ref: MockCommandExecutor,
        method: InputMethod,
        quiet: bool,
    }

    impl MockInjectorStation {
        fn new(method: InputMethod) -> Self {
            let executor = MockCommandExecutor::new();
            Self {
                injector: TextInjector::new(executor.clone()),
                executor_ref: executor,
                method,
                quiet: false,
            }
        }

        fn with_quiet(mut self, quiet: bool) -> Self {
            self.quiet = quiet;
            self
        }

        fn with_failure(method: InputMethod) -> Self {
            let executor = MockCommandExecutor::with_failure();
            Self {
                injector: TextInjector::new(executor.clone()),
                executor_ref: executor,
                method,
                quiet: false,
            }
        }

        fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.executor_ref.calls()
        }
    }

    impl Station for MockInjectorStation {
        type Input = TranscribedText;
        type Output = ();

        fn name(&self) -> &'static str {
            "mock_injector"
        }

        fn process(&mut self, text: TranscribedText) -> Result<Option<()>, StationError> {
            if !self.quiet {
                eprintln!("\"{}\"", text.text);
            }

            let result = match self.method {
                InputMethod::Clipboard => self.injector.inject_via_clipboard(&text.text),
                InputMethod::Direct => self.injector.inject_direct(&text.text),
            };

            match result {
                Ok(()) => Ok(Some(())),
                Err(e) => Err(StationError::Recoverable(format!(
                    "Injection failed: {}",
                    e
                ))),
            }
        }
    }

    #[test]
    fn test_injector_station_creation() {
        let station = InjectorStation::new(InputMethod::Clipboard);
        assert_eq!(station.name(), "injector");
        assert!(!station.quiet);
    }

    #[test]
    fn test_injector_station_with_quiet() {
        let station = InjectorStation::new(InputMethod::Clipboard).with_quiet(true);
        assert_eq!(station.name(), "injector");
        assert!(station.quiet);
    }

    #[test]
    fn test_process_via_clipboard() {
        let mut station = MockInjectorStation::new(InputMethod::Clipboard);

        let text = TranscribedText::new("Hello world".to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(()));

        let calls = station.calls();
        assert_eq!(calls.len(), 2);
        // First call: wl-copy with the text
        assert_eq!(calls[0].0, "wl-copy");
        assert_eq!(calls[0].1, vec!["Hello world"]);
        // Second call: wtype for paste
        assert_eq!(calls[1].0, "wtype");
    }

    #[test]
    fn test_process_via_direct() {
        let mut station = MockInjectorStation::new(InputMethod::Direct);

        let text = TranscribedText::new("Hello world".to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(()));

        let calls = station.calls();
        assert_eq!(calls.len(), 1);
        // Direct call: wtype with text
        assert_eq!(calls[0].0, "wtype");
        assert_eq!(calls[0].1, vec!["Hello world"]);
    }

    #[test]
    fn test_process_injection_failure() {
        let mut station = MockInjectorStation::with_failure(InputMethod::Clipboard);

        let text = TranscribedText::new("Test".to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_err());
        match result.unwrap_err() {
            StationError::Recoverable(msg) => {
                assert!(msg.contains("Injection failed"));
            }
            StationError::Fatal(_) => panic!("Expected Recoverable error"),
        }
    }

    #[test]
    fn test_quiet_mode() {
        // Test with quiet = false (default)
        let mut station_loud = MockInjectorStation::new(InputMethod::Direct);
        let text = TranscribedText::new("Loud".to_string(), Instant::now());
        // Should print to stderr (we can't easily capture this in test, but we verify it doesn't panic)
        let result = station_loud.process(text);
        assert!(result.is_ok());

        // Test with quiet = true
        let mut station_quiet = MockInjectorStation::new(InputMethod::Direct).with_quiet(true);
        let text = TranscribedText::new("Quiet".to_string(), Instant::now());
        let result = station_quiet.process(text);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_text() {
        let mut station = MockInjectorStation::new(InputMethod::Direct);
        let text = TranscribedText::new("".to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_ok());
        let calls = station.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, vec![""]);
    }

    #[test]
    fn test_special_characters() {
        let mut station = MockInjectorStation::new(InputMethod::Direct);
        let special_text = "Hello\nWorld\t!@#$%^&*()";
        let text = TranscribedText::new(special_text.to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_ok());
        let calls = station.calls();
        assert_eq!(calls[0].1, vec![special_text]);
    }

    #[test]
    fn test_unicode_text() {
        let mut station = MockInjectorStation::new(InputMethod::Direct);
        let unicode_text = "Hello 世界 🌍";
        let text = TranscribedText::new(unicode_text.to_string(), Instant::now());
        let result = station.process(text);

        assert!(result.is_ok());
        let calls = station.calls();
        assert_eq!(calls[0].1, vec![unicode_text]);
    }
}
