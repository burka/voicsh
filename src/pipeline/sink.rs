use crate::config::{InjectionBackend, InjectionMethod};
use crate::inject::injector::{CommandExecutor, SystemCommandExecutor, TextInjector};
use crate::ipc::protocol::DaemonEvent;
use crate::output::{clear_line, render_event};
use crate::pipeline::error::StationError;
use crate::pipeline::latency::{LatencyTracker, SessionContext, TranscriptionTiming};
use crate::pipeline::station::Station;
use crate::pipeline::types::{SinkEvent, TranscribedText};
#[cfg(feature = "portal")]
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Pluggable text output handler for pipeline.
/// Pairs with AudioSource for input - this handles transcription output.
pub trait TextSink: Send + 'static {
    /// Handle transcribed text. Called for each transcription result.
    fn handle(&mut self, text: &str) -> crate::error::Result<()>;

    /// Handle a sequence of events. Default implementation processes only Text events.
    fn handle_events(&mut self, events: &[SinkEvent]) -> crate::error::Result<()> {
        for event in events {
            if let SinkEvent::Text(t) = event {
                self.handle(t)?;
            }
        }
        Ok(())
    }

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
    latency_tracker: LatencyTracker,
    transcription_count: usize,
    event_tx: Option<crossbeam_channel::Sender<DaemonEvent>>,
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
            latency_tracker: LatencyTracker::new(),
            transcription_count: 0,
            event_tx: None,
        }
    }

    pub(crate) fn with_session_context(mut self, context: SessionContext) -> Self {
        self.latency_tracker = LatencyTracker::with_context(context);
        self
    }

    pub(crate) fn with_event_sender(mut self, tx: crossbeam_channel::Sender<DaemonEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }
}

impl Station for SinkStation {
    type Input = TranscribedText;
    type Output = ();

    fn name(&self) -> &'static str {
        self.sink.name()
    }

    fn process(&mut self, text: TranscribedText) -> Result<Option<()>, StationError> {
        // Skip if both text and events are empty
        if text.text.trim().is_empty() && text.events.is_empty() {
            return Ok(None);
        }

        let handle_result = if !text.events.is_empty() {
            self.sink.handle_events(&text.events)
        } else {
            self.sink.handle(&text.text)
        };

        match handle_result {
            Ok(()) => {
                let output_done = Instant::now();
                self.transcription_count += 1;

                // Compute wait time from timing if available
                let wait_ms = text
                    .timing
                    .as_ref()
                    .map(|t| output_done.duration_since(t.capture_start).as_millis() as u32);

                // Emit transcription event for follow clients (with timing)
                if let Some(ref tx) = self.event_tx
                    && tx
                        .try_send(DaemonEvent::Transcription {
                            text: text.text.clone(),
                            language: text.language.clone(),
                            confidence: text.confidence,
                            wait_ms,
                            word_probabilities: text.word_probabilities.clone(),
                        })
                        .is_err()
                {
                    // Channel full or closed - OK to ignore in sink
                }

                // Record timing and show output
                if let Some(chunk_timing) = text.timing {
                    let timing = TranscriptionTiming {
                        capture_start: chunk_timing.capture_start,
                        vad_start: chunk_timing.vad_start,
                        chunk_created: chunk_timing.chunk_created,
                        transcription_done: text.timestamp,
                        output_done,
                        audio_duration: Duration::from_millis(
                            chunk_timing.audio_duration_ms as u64,
                        ),
                    };
                    self.latency_tracker.record(timing.clone());

                    if !self.quiet {
                        // Show transcription (with inline wait) for all verbosity levels
                        render_event(&DaemonEvent::Transcription {
                            text: text.text.clone(),
                            language: text.language.clone(),
                            confidence: text.confidence,
                            wait_ms,
                            word_probabilities: text.word_probabilities.clone(),
                        });
                        // Verbose >= 2: supplementary detailed breakdown
                        if self.verbosity >= 2 {
                            self.latency_tracker.print_detailed(
                                &timing,
                                &text.text,
                                self.transcription_count,
                            );
                        }
                    }
                } else if !self.quiet && self.verbosity == 0 {
                    // No timing - just show transcription event
                    render_event(&DaemonEvent::Transcription {
                        text: text.text.clone(),
                        language: text.language.clone(),
                        confidence: text.confidence,
                        wait_ms: None,
                        word_probabilities: text.word_probabilities.clone(),
                    });
                }
                Ok(Some(()))
            }
            Err(e) => {
                if !self.quiet {
                    if self.verbosity >= 1 {
                        clear_line();
                        eprintln!("[FAIL {}] \"{}\"", e, text.text);
                    } else {
                        clear_line();
                        eprintln!("\"{}\"", text.text);
                    }
                }
                Ok(None)
            }
        }
    }

    fn shutdown(&mut self) {
        // Print latency summary if we have measurements and verbosity is enabled
        if !self.quiet && self.verbosity >= 1 {
            self.latency_tracker.print_summary();
        }

        let result = self.sink.finish();
        if let Some(tx) = self.result_tx.take()
            && tx.send(result).is_err()
        {
            eprintln!("voicsh: sink shutdown — result receiver already dropped");
        }
    }
}

/// Voice typing sink - injects text via clipboard or direct input.
/// Extracted from InjectorStation for modularity.
pub struct InjectorSink<E: CommandExecutor> {
    injector: TextInjector<E>,
    method: InjectionMethod,
    paste_key: String,
    verbosity: u8,
}

impl InjectorSink<SystemCommandExecutor> {
    /// Create InjectorSink with system command executor (production use).
    pub fn system(
        method: InjectionMethod,
        paste_key: String,
        verbosity: u8,
        backend: InjectionBackend,
    ) -> Self {
        Self {
            injector: TextInjector::system().with_backend(backend),
            method,
            paste_key,
            verbosity,
        }
    }

    /// Create InjectorSink with portal session for key injection.
    #[cfg(feature = "portal")]
    pub fn with_portal(
        method: InjectionMethod,
        paste_key: String,
        verbosity: u8,
        portal: Option<Arc<crate::inject::portal::PortalSession>>,
        backend: InjectionBackend,
    ) -> Self {
        Self {
            injector: TextInjector::system()
                .with_portal(portal)
                .with_backend(backend),
            method,
            paste_key,
            verbosity,
        }
    }
}

impl<E: CommandExecutor> InjectorSink<E> {
    /// Create InjectorSink with custom executor (testing/library use).
    pub fn new(injector: TextInjector<E>, method: InjectionMethod, paste_key: String) -> Self {
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
        // Normalize: trim trailing whitespace, append exactly one space so
        // consecutive dictations flow naturally ("word1 word2 " not "word1word2").
        let normalized = format!("{} ", text.trim_end());

        let paste_key =
            crate::inject::focused_window::resolve_paste_key(&self.paste_key, self.verbosity);

        match self.method {
            InjectionMethod::Clipboard => {
                self.injector.inject_via_clipboard(&normalized, paste_key)?;
            }
            InjectionMethod::Direct => {
                self.injector.inject_direct(&normalized)?;
            }
        }

        Ok(())
    }

    fn handle_events(&mut self, events: &[SinkEvent]) -> crate::error::Result<()> {
        let paste_key =
            crate::inject::focused_window::resolve_paste_key(&self.paste_key, self.verbosity);

        for event in events {
            match event {
                SinkEvent::Text(text) => {
                    let normalized = format!("{} ", text.trim_end());
                    match self.method {
                        InjectionMethod::Clipboard => {
                            self.injector.inject_via_clipboard(&normalized, paste_key)?;
                        }
                        InjectionMethod::Direct => {
                            self.injector.inject_direct(&normalized)?;
                        }
                    }
                }
                SinkEvent::KeyCombo(combo) => {
                    self.injector.inject_key_combo(combo)?;
                }
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
    use crate::config::InjectionBackend;
    use std::sync::{Arc, Mutex};

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
        let sink = InjectorSink::system(
            InjectionMethod::Clipboard,
            "ctrl+v".to_string(),
            0,
            InjectionBackend::Auto,
        );
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
        let mut sink =
            InjectorSink::new(injector, InjectionMethod::Clipboard, "ctrl+v".to_string());

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
        let mut sink = InjectorSink::new(injector, InjectionMethod::Direct, "ctrl+v".to_string());

        sink.handle("Direct text").unwrap();

        let commands = executor.commands();
        // Text is normalized with trailing space for natural dictation flow
        assert!(
            commands
                .iter()
                .any(|c| (c.contains("wtype") || c.contains("ydotool"))
                    && c.contains("Direct text "))
        );
    }

    #[test]
    fn injector_sink_normalizes_trailing_whitespace() {
        let executor = MockCommandExecutor::new();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InjectionMethod::Direct, "ctrl+v".to_string());

        // Input with extra trailing whitespace → trimmed to exactly one space
        sink.handle("hello   ").unwrap();

        let commands = executor.commands();
        assert!(
            commands.iter().any(|c| c.contains("hello ")),
            "Should have exactly one trailing space, got: {:?}",
            commands
        );
        assert!(
            !commands.iter().any(|c| c.contains("hello  ")),
            "Should not have multiple trailing spaces, got: {:?}",
            commands
        );
    }

    #[test]
    fn injector_sink_failure_propagates() {
        let executor = MockCommandExecutor::new();
        executor.set_fail_next();
        let injector = TextInjector::new(executor.clone());
        let mut sink =
            InjectorSink::new(injector, InjectionMethod::Clipboard, "ctrl+v".to_string());

        let result = sink.handle("Test");
        assert!(result.is_err());
    }

    #[test]
    fn sink_station_delegates_to_sink() {
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let text1 = TranscribedText::new("First".to_string());
        let text2 = TranscribedText::new("Second".to_string());

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

        let empty_text = TranscribedText::new("   ".to_string());

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
        let sink = InjectorSink::new(injector, InjectionMethod::Clipboard, "ctrl+v".to_string());

        let (result_tx, _result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(sink), true, 0, result_tx);

        let text = TranscribedText::new("Test".to_string());

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
        let sink = InjectorSink::system(
            InjectionMethod::Clipboard,
            "ctrl+v".to_string(),
            0,
            InjectionBackend::Auto,
        );
        assert_eq!(sink.name(), "injector");
    }

    #[test]
    fn stdout_sink_name() {
        let sink = StdoutSink;
        assert_eq!(sink.name(), "stdout");
    }

    #[test]
    fn test_sink_shutdown_logs_on_send_failure() {
        // Drop the receiver before shutdown so tx.send() fails.
        // Verifies no panic — the error path logs via eprintln.
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);

        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let text = TranscribedText::new("before shutdown".to_string());
        let processed = station.process(text).unwrap();
        assert_eq!(processed, Some(()));

        // Drop receiver so shutdown's send() will fail
        drop(result_rx);

        // shutdown() should log the failure but not panic
        station.shutdown();
    }

    // ── SinkEvent tests ──────────────────────────────────────────────────

    #[test]
    fn test_handle_events_routes_text_events_correctly() {
        let mut sink = CollectorSink::new();

        let events = vec![
            SinkEvent::Text("hello".to_string()),
            SinkEvent::Text("world".to_string()),
        ];

        sink.handle_events(&events).unwrap();

        let result = sink.finish();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_handle_events_default_skips_key_combo() {
        let mut sink = CollectorSink::new();

        let events = vec![
            SinkEvent::Text("hello".to_string()),
            SinkEvent::KeyCombo("ctrl+BackSpace".to_string()),
            SinkEvent::Text("world".to_string()),
        ];

        sink.handle_events(&events).unwrap();

        let result = sink.finish();
        // KeyCombo should be skipped by default implementation
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_sink_station_uses_handle_events_when_events_present() {
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let mut text = TranscribedText::new("".to_string());
        text.events = vec![
            SinkEvent::Text("from".to_string()),
            SinkEvent::Text("events".to_string()),
        ];

        station.process(text).unwrap();
        station.shutdown();

        let result = result_rx.recv().unwrap();
        assert_eq!(result, Some("from events".to_string()));
    }

    #[test]
    fn test_sink_station_uses_handle_when_events_empty() {
        let collector = CollectorSink::new();
        let (result_tx, result_rx) = crossbeam_channel::bounded(1);
        let mut station = SinkStation::new(Box::new(collector), true, 0, result_tx);

        let text = TranscribedText::new("from text field".to_string());
        // events is empty, so should use handle() with text.text

        station.process(text).unwrap();
        station.shutdown();

        let result = result_rx.recv().unwrap();
        assert_eq!(result, Some("from text field".to_string()));
    }

    #[test]
    fn injector_sink_handle_events_text() {
        use crate::pipeline::types::SinkEvent;
        let executor = MockCommandExecutor::new();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InjectionMethod::Direct, "ctrl+v".to_string());

        let events = vec![
            SinkEvent::Text("hello".to_string()),
            SinkEvent::Text("world".to_string()),
        ];
        sink.handle_events(&events).unwrap();

        let commands = executor.commands();
        // Each text event produces a wtype call with normalized text
        assert!(commands.iter().any(|c| c.contains("hello ")));
        assert!(commands.iter().any(|c| c.contains("world ")));
    }

    #[test]
    fn injector_sink_handle_events_key_combo() {
        use crate::pipeline::types::SinkEvent;
        let executor = MockCommandExecutor::new();
        let injector = TextInjector::new(executor.clone());
        let mut sink = InjectorSink::new(injector, InjectionMethod::Direct, "ctrl+v".to_string());

        let events = vec![
            SinkEvent::Text("hello ".to_string()),
            SinkEvent::KeyCombo("ctrl+BackSpace".to_string()),
            SinkEvent::Text("world".to_string()),
        ];
        sink.handle_events(&events).unwrap();

        let commands = executor.commands();
        // Should have: wtype for "hello ", wtype for key combo, wtype for "world "
        assert!(
            commands.len() >= 3,
            "Expected at least 3 commands, got: {:?}",
            commands
        );
        // Key combo should produce wtype with -M and -k args
        assert!(
            commands.iter().any(|c| c.contains("wtype")
                && c.contains("-M")
                && c.contains("ctrl")
                && c.contains("BackSpace")),
            "Expected key combo wtype call, got: {:?}",
            commands
        );
    }
}
