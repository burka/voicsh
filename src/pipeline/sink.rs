use crate::config::InputMethod;
use crate::input::injector::{CommandExecutor, SystemCommandExecutor, TextInjector};
use crate::pipeline::error::{StationError, eprintln_clear};
use crate::pipeline::station::Station;
use crate::pipeline::types::TranscribedText;
#[cfg(feature = "portal")]
use std::sync::Arc;

/// Pluggable text output handler for pipeline.
/// Pairs with AudioSource for input - this handles transcription output.
pub trait TextSink: Send + 'static {
    /// Handle transcribed text. Called for each transcription result.
    fn handle(&mut self, text: &str) -> crate::error::Result<()>;

    /// Called on pipeline shutdown. Return accumulated text if applicable.
    fn finish(&mut self) -> Option<String> {
        None
    }

    /// Name for logging/debugging.
    fn name(&self) -> &'static str {
        "sink"
    }
}

/// Station wrapper for any TextSink implementation.
/// Converts TextSink into a Station for pipeline orchestration.
#[allow(dead_code)]
pub(crate) struct SinkStation {
    sink: Box<dyn TextSink>,
    quiet: bool,
    verbosity: u8,
    result_tx: Option<crossbeam_channel::Sender<Option<String>>>,
}

#[allow(dead_code)]
impl SinkStation {
    pub(crate) fn new(
        sink: Box<dyn TextSink>,
        quiet: bool,
        verbosity: u8,
        result_tx: crossbeam_channel::Sender<Option<String>>,
    ) -> Self {
        Self {
            sink,
            quiet,
            verbosity,
            result_tx: Some(result_tx),
        }
    }
}

impl Station for SinkStation {
    type Input = TranscribedText;
    type Output = ();

    fn name(&self) -> &'static str {
        self.sink.name()
    }

    fn process(&mut self, text: TranscribedText) -> Result<Option<()>, StationError> {
        if text.text.trim().is_empty() {
            return Ok(None);
        }

        match self.sink.handle(&text.text) {
            Ok(()) => {
                if !self.quiet {
                    if self.verbosity >= 1 {
                        let chars = text.text.len();
                        eprintln_clear(&format!("[ok {}ch] \"{}\"", chars, text.text));
                    } else {
                        eprintln_clear(&format!("\"{}\"", text.text));
                    }
                }
                Ok(Some(()))
            }
            Err(e) => {
                if !self.quiet {
                    if self.verbosity >= 1 {
                        eprintln_clear(&format!("[FAIL {}] \"{}\"", e, text.text));
                    } else {
                        eprintln_clear(&format!("\"{}\"", text.text));
                    }
                }
                Ok(None)
            }
        }
    }

    fn shutdown(&mut self) {
        let result = self.sink.finish();
        if let Some(tx) = self.result_tx.take() {
            let _ = tx.send(result);
        }
    }
}

/// Voice typing sink - injects text via clipboard or direct input.
/// Extracted from InjectorStation for modularity.
pub struct InjectorSink<E: CommandExecutor> {
    injector: TextInjector<E>,
    method: InputMethod,
    paste_key: String,
    verbosity: u8,
}

impl InjectorSink<SystemCommandExecutor> {
    /// Create InjectorSink with system command executor (production use).
    pub fn system(method: InputMethod, paste_key: String, verbosity: u8) -> Self {
        Self {
            injector: TextInjector::system(),
            method,
            paste_key,
            verbosity,
        }
    }

    /// Create InjectorSink with portal session for key injection.
    #[cfg(feature = "portal")]
    pub fn with_portal(
        method: InputMethod,
        paste_key: String,
        verbosity: u8,
        portal: Option<Arc<crate::input::portal::PortalSession>>,
    ) -> Self {
        Self {
            injector: TextInjector::system().with_portal(portal),
            method,
            paste_key,
            verbosity,
        }
    }
}

impl<E: CommandExecutor> InjectorSink<E> {
    /// Create InjectorSink with custom executor (testing/library use).
    pub fn new(injector: TextInjector<E>, method: InputMethod, paste_key: String) -> Self {
        Self {
            injector,
            method,
            paste_key,
            verbosity: 0,
        }
    }
}

impl<E: CommandExecutor + 'static> TextSink for InjectorSink<E> {
    fn handle(&mut self, text: &str) -> crate::error::Result<()> {
        let paste_key =
            crate::input::focused_window::resolve_paste_key(&self.paste_key, self.verbosity);

        match self.method {
            InputMethod::Clipboard => {
                self.injector.inject_via_clipboard(text, paste_key)?;
            }
            InputMethod::Direct => {
                self.injector.inject_direct(text)?;
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "injector"
    }
}

/// Collects transcribed text for --once mode and library use.
/// Returns accumulated text on finish().
pub struct CollectorSink {
    collected: Vec<String>,
}

impl CollectorSink {
    pub fn new() -> Self {
        Self {
            collected: Vec::new(),
        }
    }
}

impl Default for CollectorSink {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSink for CollectorSink {
    fn handle(&mut self, text: &str) -> crate::error::Result<()> {
        self.collected.push(text.to_string());
        Ok(())
    }

    fn finish(&mut self) -> Option<String> {
        if self.collected.is_empty() {
            None
        } else {
            Some(self.collected.join(" "))
        }
    }

    fn name(&self) -> &'static str {
        "collector"
    }
}

/// Pipe mode sink — writes transcribed text to stdout.
pub struct StdoutSink;

impl TextSink for StdoutSink {
    fn handle(&mut self, text: &str) -> crate::error::Result<()> {
        println!("{}", text);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "stdout"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    #[test]
    fn text_sink_is_object_safe() {
        let _sink: Box<dyn TextSink> = Box::new(CollectorSink::new());
    }

    #[test]
    fn collector_sink_collects_and_joins_text() {
        let mut sink = CollectorSink::new();

        sink.handle("Hello").unwrap();
        sink.handle("world").unwrap();
        sink.handle("Rust").unwrap();

        let result = sink.finish();
        assert_eq!(result, Some("Hello world Rust".to_string()));
    }

    #[test]
    fn collector_sink_empty_returns_none() {
        let mut sink = CollectorSink::new();
        let result = sink.finish();
        assert_eq!(result, None);
    }

    #[test]
    fn collector_sink_single_item() {
        let mut sink = CollectorSink::new();
        sink.handle("Single").unwrap();
        let result = sink.finish();
        assert_eq!(result, Some("Single".to_string()));
    }

    #[test]
    fn injector_sink_system_constructor() {
        let sink = InjectorSink::system(InputMethod::Clipboard, "ctrl+v".to_string(), 0);
        assert_eq!(sink.name(), "injector");
    }

    // Mock executor for testing
    #[derive(Clone)]
    struct MockCommandExecutor {
        commands: Arc<Mutex<Vec<String>>>,
        fail_next: Arc<Mutex<bool>>,
    }

    impl MockCommandExecutor {
        fn new() -> Self {
            Self {
                commands: Arc::new(Mutex::new(Vec::new())),
                fail_next: Arc::new(Mutex::new(false)),
            }
        }

        fn commands(&self) -> Vec<String> {
            self.commands.lock().unwrap().clone()
        }

        fn set_fail_next(&self) {
            *self.fail_next.lock().unwrap() = true;
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn execute(&self, command: &str, args: &[&str]) -> crate::error::Result<()> {
            if *self.fail_next.lock().unwrap() {
                *self.fail_next.lock().unwrap() = false;
                return Err(crate::error::VoicshError::InjectionFailed {
                    message: "mock failure".to_string(),
                });
            }

            let full_cmd = format!("{} {}", command, args.join(" "));
            self.commands.lock().unwrap().push(full_cmd);
            Ok(())
        }
    }

    #[test]
    fn injector_sink_handles_clipboard_injection() {
        let executor = MockCommandExecutor::new();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InputMethod::Clipboard, "ctrl+v".to_string());

        sink.handle("Test text").unwrap();

        let commands = executor.commands();
        assert!(commands.len() >= 2);
        assert!(commands.iter().any(|c| c.contains("wl-copy")));
        assert!(
            commands
                .iter()
                .any(|c| c.contains("wtype") || c.contains("ydotool"))
        );
    }

    #[test]
    fn injector_sink_handles_direct_injection() {
        let executor = MockCommandExecutor::new();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InputMethod::Direct, "ctrl+v".to_string());

        sink.handle("Direct text").unwrap();

        let commands = executor.commands();
        assert!(
            commands.iter().any(
                |c| (c.contains("wtype") || c.contains("ydotool")) && c.contains("Direct text")
            )
        );
    }

    #[test]
    fn injector_sink_failure_propagates() {
        let executor = MockCommandExecutor::new();
        executor.set_fail_next();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InputMethod::Clipboard, "ctrl+v".to_string());

        let result = sink.handle("Test");
        assert!(result.is_err());
    }

    #[test]
    fn sink_station_delegates_to_sink() {
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let text1 = TranscribedText {
            text: "First".to_string(),
            timestamp: Instant::now(),
        };
        let text2 = TranscribedText {
            text: "Second".to_string(),
            timestamp: Instant::now(),
        };

        station.process(text1).unwrap();
        station.process(text2).unwrap();
        station.shutdown();

        let result = result_rx.recv().unwrap();
        assert_eq!(result, Some("First Second".to_string()));
    }

    #[test]
    fn sink_station_skips_empty_text() {
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let empty_text = TranscribedText {
            text: "   ".to_string(),
            timestamp: Instant::now(),
        };

        let result = station.process(empty_text).unwrap();
        assert_eq!(result, None);

        station.shutdown();
        let result = result_rx.recv().unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn sink_station_continues_on_sink_failure() {
        let executor = MockCommandExecutor::new();
        executor.set_fail_next();
        let injector = TextInjector::new(executor.clone());
        let sink = InjectorSink::new(injector, InputMethod::Clipboard, "ctrl+v".to_string());

        let (result_tx, _result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(sink), true, 0, result_tx);

        let text = TranscribedText {
            text: "Test".to_string(),
            timestamp: Instant::now(),
        };

        let result = station.process(text);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn sink_station_name_delegates_to_sink() {
        let collector = CollectorSink::new();
        let (result_tx, _result_rx) = crossbeam_channel::bounded(1);
        let station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        assert_eq!(station.name(), "collector");
    }

    #[test]
    fn collector_sink_name() {
        let sink = CollectorSink::new();
        assert_eq!(sink.name(), "collector");
    }

    #[test]
    fn injector_sink_name() {
        let sink = InjectorSink::system(InputMethod::Clipboard, "ctrl+v".to_string(), 0);
        assert_eq!(sink.name(), "injector");
    }

    #[test]
    fn stdout_sink_name() {
        let sink = StdoutSink;
        assert_eq!(sink.name(), "stdout");
    }
}
