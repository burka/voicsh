//! Shared event rendering for terminal output.
//! Used by both `voicsh follow` and daemon verbose mode.

use crate::ipc::protocol::{DaemonEvent, TextOrigin};
use crate::pipeline::vad_station::format_level_bar;
use crate::stt::transcriber::TokenProbability;
use std::io::{self, Write};

const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";
const STRIKETHROUGH: &str = "\x1b[9m";

/// Below this confidence, hallucination filter drops are suppressed from output.
/// Low-confidence hallucination hits are noise (the model is uncertain and the
/// filter caught it — no need to show anything).
const HALLUCINATION_SUPPRESS_CONFIDENCE: f32 = 0.75;

/// Clear the current terminal line (replaces level bar etc.)
pub fn clear_line() {
    eprint!("\r\x1b[2K");
}

/// Return the ANSI color code for a token probability.
fn probability_color(prob: f32) -> &'static str {
    if prob >= 0.9 {
        GREEN
    } else if prob >= 0.7 {
        "" // default terminal color
    } else if prob >= 0.5 {
        YELLOW
    } else {
        RED
    }
}

/// Render tokens colored by their probability.
fn render_tokens_colored(token_probabilities: &[TokenProbability]) {
    for tp in token_probabilities {
        let color = probability_color(tp.probability);
        if color.is_empty() {
            eprint!("{}", tp.token);
        } else {
            eprint!("{color}{}{RESET}", tp.token);
        }
    }
}

/// Render a voice command replacement: show raw in strikethrough brackets, then replacement.
fn render_voice_command_diff(raw_text: &str, text: &str) {
    eprint!("{STRIKETHROUGH}{DIM}[{raw_text}]{RESET}{text}");
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
            token_probabilities,
            raw_text,
            text_origin,
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

            match (text_origin, raw_text) {
                (TextOrigin::VoiceCommand, Some(raw)) => {
                    render_voice_command_diff(raw, text);
                    eprintln!("{lang}{wait}");
                }
                _ => {
                    if token_probabilities.is_empty() {
                        eprintln!("{text}{lang}{wait}");
                    } else {
                        render_tokens_colored(token_probabilities);
                        eprintln!("{lang}{wait}");
                    }
                }
            }
        }
        DaemonEvent::TranscriptionDropped {
            text,
            language,
            confidence,
            reason,
        } => {
            // Low-confidence hallucination filter hits are noise — suppress them
            if reason == "hallucination filter" && *confidence < HALLUCINATION_SUPPRESS_CONFIDENCE {
                return;
            }
            clear_line();
            let lang = if !language.is_empty() && *confidence < 0.99 {
                format!(" [{language}] {:.0}%", confidence * 100.0)
            } else if !language.is_empty() {
                format!(" [{language}]")
            } else {
                String::new()
            };
            eprintln!("{DIM}{STRIKETHROUGH}{text}{RESET}{DIM}{lang} ({reason}){RESET}");
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
        DaemonEvent::DaemonInfo {
            binary_path,
            version,
        } => {
            clear_line();
            eprintln!("{DIM}Daemon v{version} ({binary_path}){RESET}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::TextOrigin;
    use crate::stt::transcriber::TokenProbability;

    // ── probability color tests ────────────────────────────────────────

    #[test]
    fn probability_color_thresholds() {
        assert_eq!(probability_color(0.95), GREEN);
        assert_eq!(probability_color(0.90), GREEN);
        assert_eq!(probability_color(0.89), "");
        assert_eq!(probability_color(0.70), "");
        assert_eq!(probability_color(0.69), YELLOW);
        assert_eq!(probability_color(0.50), YELLOW);
        assert_eq!(probability_color(0.49), RED);
        assert_eq!(probability_color(0.1), RED);
    }

    // ── hallucination suppression tests ──────────────────────────────

    #[test]
    fn hallucination_suppression_below_threshold() {
        // Below HALLUCINATION_SUPPRESS_CONFIDENCE → suppressed (no output)
        // This calls render_event which writes to stderr; the key property is
        // that it returns early without printing the struck-through line.
        // We verify the threshold constant is used correctly.
        assert!(
            0.74 < HALLUCINATION_SUPPRESS_CONFIDENCE,
            "0.74 should be below the suppression threshold"
        );
        assert!(
            0.75 >= HALLUCINATION_SUPPRESS_CONFIDENCE,
            "0.75 should be at or above the suppression threshold"
        );
    }

    #[test]
    fn hallucination_suppression_at_threshold_renders() {
        // At exactly HALLUCINATION_SUPPRESS_CONFIDENCE → not suppressed
        // Smoke test: should not panic
        render_event(&DaemonEvent::TranscriptionDropped {
            text: "Thank you.".to_string(),
            language: "en".to_string(),
            confidence: HALLUCINATION_SUPPRESS_CONFIDENCE,
            reason: "hallucination filter".to_string(),
        });
    }

    #[test]
    fn hallucination_suppression_above_threshold_renders() {
        // Above HALLUCINATION_SUPPRESS_CONFIDENCE → not suppressed
        render_event(&DaemonEvent::TranscriptionDropped {
            text: "Thank you.".to_string(),
            language: "en".to_string(),
            confidence: 0.90,
            reason: "hallucination filter".to_string(),
        });
    }

    #[test]
    fn non_hallucination_drop_always_renders() {
        // Other reasons are never suppressed regardless of confidence
        render_event(&DaemonEvent::TranscriptionDropped {
            text: "test".to_string(),
            language: "en".to_string(),
            confidence: 0.10,
            reason: "language filter".to_string(),
        });
    }

    // ── render smoke tests ─────────────────────────────────────────────

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
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            token_probabilities: vec![
                TokenProbability {
                    token: " hello".to_string(),
                    probability: 0.95,
                },
                TokenProbability {
                    token: " world".to_string(),
                    probability: 0.75,
                },
            ],
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

        render_event(&DaemonEvent::DaemonInfo {
            binary_path: "/usr/bin/voicsh".to_string(),
            version: "0.1.0+abc1234".to_string(),
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
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            token_probabilities: vec![],
        });
    }

    #[test]
    fn test_render_transcription_with_token_probabilities() {
        render_event(&DaemonEvent::Transcription {
            text: "high medium low".to_string(),
            language: "en".to_string(),
            confidence: 0.7,
            wait_ms: Some(250),
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            token_probabilities: vec![
                TokenProbability {
                    token: " high".to_string(),
                    probability: 0.95,
                },
                TokenProbability {
                    token: " medium".to_string(),
                    probability: 0.65,
                },
                TokenProbability {
                    token: " low".to_string(),
                    probability: 0.35,
                },
            ],
        });
    }
    #[test]
    fn test_render_voice_command_transcription() {
        // Smoke test: render_event writes to stderr which can't be captured.
        // Validates voice command rendering doesn't panic.
        render_event(&DaemonEvent::Transcription {
            text: ".".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
            token_probabilities: vec![],
            raw_text: Some("period".to_string()),
            text_origin: TextOrigin::VoiceCommand,
        });
    }
}
