//! Shared event rendering for terminal output.
//! Used by both `voicsh follow` and daemon verbose mode.

use crate::ipc::protocol::DaemonEvent;
use crate::pipeline::vad_station::format_level_bar;
use std::io::{self, Write};

const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// Clear the current terminal line (replaces level bar etc.)
pub fn clear_line() {
    eprint!("\r\x1b[2K");
}

/// Render a daemon event to stderr.
pub fn render_event(event: &DaemonEvent) {
    match event {
        DaemonEvent::Level {
            level,
            threshold,
            is_speech,
            buffer_used,
            buffer_capacity,
        } => {
            let bar = format_level_bar(*level, *threshold);
            let speech = if *is_speech { " SPEECH" } else { "" };
            let buf = if *buffer_capacity > 0 {
                format!("  {DIM}buf {buffer_used}/{buffer_capacity}{RESET}")
            } else {
                String::new()
            };
            eprint!("\r\x1b[2K{bar}{speech}{buf}");
            io::stderr().flush().ok();
        }
        DaemonEvent::RecordingStateChanged { recording } => {
            clear_line();
            if *recording {
                eprintln!("Recording started");
            } else {
                eprintln!("Recording stopped");
            }
        }
        DaemonEvent::Transcription {
            text,
            language,
            confidence,
            wait_ms,
        } => {
            clear_line();
            let lang = if !language.is_empty() && *confidence < 0.99 {
                format!(" {DIM}[{language}] {:.0}%{RESET}", confidence * 100.0)
            } else if !language.is_empty() {
                format!(" {DIM}[{language}]{RESET}")
            } else {
                String::new()
            };
            let wait = wait_ms
                .map(|ms| format!(" {DIM}({ms}ms){RESET}"))
                .unwrap_or_default();
            eprintln!("{text}{lang}{wait}");
        }
        DaemonEvent::TranscriptionDropped {
            text,
            language,
            confidence,
            reason,
        } => {
            clear_line();
            let conf = if *confidence < 0.99 {
                format!(" {:.0}%", confidence * 100.0)
            } else {
                String::new()
            };
            eprintln!("{DIM}[dropped: {reason} | {language}{conf}] \"{text}\"{RESET}");
        }
        DaemonEvent::Log { message } => {
            clear_line();
            eprintln!("{DIM}[log] {message}{RESET}");
        }
        DaemonEvent::ConfigChanged { key, value } => {
            clear_line();
            eprintln!("Config changed: {key} = {value}");
        }
        DaemonEvent::ModelLoading { model, progress } => {
            eprint!("\r\x1b[2KModel {model}: {progress}...");
            io::stderr().flush().ok();
        }
        DaemonEvent::ModelLoaded { model } => {
            clear_line();
            eprintln!("{GREEN}Model {model} loaded{RESET}");
        }
        DaemonEvent::ModelLoadFailed { model, error } => {
            clear_line();
            eprintln!("{RED}Model {model} failed: {error}{RESET}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_event_doesnt_panic() {
        // Smoke test: render_event writes to stderr which can't be captured in tests.
        // Validates all variants render without panicking.
        render_event(&DaemonEvent::Level {
            level: 0.15,
            threshold: 0.08,
            is_speech: true,
            buffer_used: 3,
            buffer_capacity: 8,
        });

        render_event(&DaemonEvent::RecordingStateChanged { recording: true });

        render_event(&DaemonEvent::Transcription {
            text: "hello world".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
        });

        render_event(&DaemonEvent::TranscriptionDropped {
            text: "test".to_string(),
            language: "ru".to_string(),
            confidence: 0.3,
            reason: "language filter".to_string(),
        });

        render_event(&DaemonEvent::Log {
            message: "test message".to_string(),
        });

        render_event(&DaemonEvent::ConfigChanged {
            key: "language".to_string(),
            value: "de".to_string(),
        });

        render_event(&DaemonEvent::ModelLoading {
            model: "base".to_string(),
            progress: "downloading".to_string(),
        });

        render_event(&DaemonEvent::ModelLoaded {
            model: "base".to_string(),
        });

        render_event(&DaemonEvent::ModelLoadFailed {
            model: "base".to_string(),
            error: "download failed".to_string(),
        });
    }

    // Smoke test: stderr output can't be captured.
    #[test]
    fn test_clear_line_doesnt_panic() {
        clear_line();
    }

    #[test]
    fn test_render_level_without_buffer() {
        render_event(&DaemonEvent::Level {
            level: 0.1,
            threshold: 0.05,
            is_speech: false,
            buffer_used: 0,
            buffer_capacity: 0,
        });
    }

    #[test]
    fn test_render_transcription_without_language() {
        render_event(&DaemonEvent::Transcription {
            text: "test".to_string(),
            language: String::new(),
            confidence: 0.9,
            wait_ms: None,
        });
    }
}
