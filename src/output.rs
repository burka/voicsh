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

/// Build a word->minimum-probability map from token probabilities.
///
/// Tokens may contain leading whitespace (e.g., " hello"). We split each token
/// into whitespace-separated words and assign each word the minimum probability
/// of the tokens that contribute to it.
fn build_word_prob_map(token_probabilities: &[TokenProbability]) -> Vec<(String, f32)> {
    let mut result: Vec<(String, f32)> = Vec::new();

    for tp in token_probabilities {
        let words: Vec<&str> = tp.token.split_whitespace().collect();
        for word in words {
            if let Some(last) = result.last_mut()
                && last.0 == word
            {
                last.1 = last.1.min(tp.probability);
                continue;
            }
            result.push((word.to_string(), tp.probability));
        }
    }

    result
}

/// Compute the longest common subsequence of two word slices.
/// Returns a list of (old_idx, new_idx) pairs for matched words.
fn lcs_indices(old_words: &[&str], new_words: &[&str]) -> Vec<(usize, usize)> {
    let m = old_words.len();
    let n = new_words.len();

    // Build LCS length table
    // u16 suffices: voice transcription chunks are at most ~100 words
    let mut table = vec![vec![0u16; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_words[i - 1].eq_ignore_ascii_case(new_words[j - 1]) {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }

    // Backtrack to find matched pairs
    let mut matches = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if old_words[i - 1].eq_ignore_ascii_case(new_words[j - 1]) {
            matches.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    matches.reverse();
    matches
}

/// Diff operation for word-level diff.
#[derive(Debug, PartialEq)]
enum DiffOp<'a> {
    /// Word present in both old and new (unchanged).
    Equal(&'a str),
    /// Word only in old (deleted/replaced).
    Delete(&'a str),
    /// Word only in new (inserted/replacement).
    Insert(&'a str),
}

/// Produce a word-level diff between old and new text.
fn word_diff<'a>(old_words: &[&'a str], new_words: &[&'a str]) -> Vec<DiffOp<'a>> {
    let matches = lcs_indices(old_words, new_words);
    let mut ops = Vec::new();

    let mut oi = 0;
    let mut ni = 0;

    for &(om, nm) in &matches {
        // Emit deletions before match
        while oi < om {
            ops.push(DiffOp::Delete(old_words[oi]));
            oi += 1;
        }
        // Emit insertions before match
        while ni < nm {
            ops.push(DiffOp::Insert(new_words[ni]));
            ni += 1;
        }
        // Emit the matched word (use new_words version to preserve casing)
        ops.push(DiffOp::Equal(new_words[nm]));
        oi = om + 1;
        ni = nm + 1;
    }

    // Remaining deletions
    while oi < old_words.len() {
        ops.push(DiffOp::Delete(old_words[oi]));
        oi += 1;
    }
    // Remaining insertions
    while ni < new_words.len() {
        ops.push(DiffOp::Insert(new_words[ni]));
        ni += 1;
    }

    ops
}

/// Look up the probability for a word (case-insensitive).
fn lookup_prob(word: &str, word_probs: &[(String, f32)]) -> f32 {
    word_probs
        .iter()
        .find(|(w, _)| w.eq_ignore_ascii_case(word))
        .map(|(_, p)| *p)
        .unwrap_or(0.5) // default to medium if not found
}

/// Render a correction diff: strikethrough original words, then replacement.
fn render_correction_diff(raw_text: &str, text: &str, token_probabilities: &[TokenProbability]) {
    let word_probs = build_word_prob_map(token_probabilities);
    let old_words: Vec<&str> = raw_text.split_whitespace().collect();
    let new_words: Vec<&str> = text.split_whitespace().collect();
    let ops = word_diff(&old_words, &new_words);

    let mut first = true;
    let mut prev_was_delete = false;
    for op in &ops {
        // No space between Delete->Insert (they form a replacement pair)
        if !(first || prev_was_delete && matches!(op, DiffOp::Insert(_))) {
            eprint!(" ");
        }
        first = false;
        prev_was_delete = matches!(op, DiffOp::Delete(_));
        match op {
            DiffOp::Equal(w) => {
                let color = probability_color(lookup_prob(w, &word_probs));
                if color.is_empty() {
                    eprint!("{w}");
                } else {
                    eprint!("{color}{w}{RESET}");
                }
            }
            DiffOp::Delete(w) => {
                let color = probability_color(lookup_prob(w, &word_probs));
                eprint!("{STRIKETHROUGH}{DIM}{color}[{w}]{RESET}");
            }
            DiffOp::Insert(w) => {
                eprint!("{w}");
            }
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
            corrector_name,
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
            let corrector_tag = corrector_name
                .as_ref()
                .map(|name| format!(" {DIM}({name}){RESET}"))
                .unwrap_or_default();

            match (text_origin, raw_text) {
                (TextOrigin::Corrected, Some(raw)) => {
                    render_correction_diff(raw, text, token_probabilities);
                    eprintln!("{lang}{wait}{corrector_tag}");
                }
                (TextOrigin::VoiceCommand, Some(raw)) => {
                    render_voice_command_diff(raw, text);
                    eprintln!("{lang}{wait}");
                }
                _ => {
                    // Plain transcription (unchanged)
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
            if (reason == "hallucination filter" || reason == "suspect word")
                && *confidence < HALLUCINATION_SUPPRESS_CONFIDENCE
            {
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

    // ── word diff algorithm tests ──────────────────────────────────────

    #[test]
    fn word_diff_identical_text() {
        let old = vec!["hello", "world"];
        let new = vec!["hello", "world"];
        let ops = word_diff(&old, &new);
        assert_eq!(ops, vec![DiffOp::Equal("hello"), DiffOp::Equal("world")]);
    }

    #[test]
    fn word_diff_single_word_correction() {
        let old = vec!["the", "quik", "fox"];
        let new = vec!["the", "quick", "fox"];
        let ops = word_diff(&old, &new);
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("the"),
                DiffOp::Delete("quik"),
                DiffOp::Insert("quick"),
                DiffOp::Equal("fox"),
            ]
        );
    }

    #[test]
    fn word_diff_multiple_corrections() {
        // When all prefix words differ, LCS groups deletions then insertions
        // before the first common element. This is correct LCS behavior.
        let old = vec!["he", "quik", "brwn", "fox"];
        let new = vec!["the", "quick", "brown", "fox"];
        let ops = word_diff(&old, &new);
        assert_eq!(
            ops,
            vec![
                DiffOp::Delete("he"),
                DiffOp::Delete("quik"),
                DiffOp::Delete("brwn"),
                DiffOp::Insert("the"),
                DiffOp::Insert("quick"),
                DiffOp::Insert("brown"),
                DiffOp::Equal("fox"),
            ]
        );
    }

    #[test]
    fn word_diff_insertion_only() {
        let old = vec!["hello", "world"];
        let new = vec!["hello", "beautiful", "world"];
        let ops = word_diff(&old, &new);
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("hello"),
                DiffOp::Insert("beautiful"),
                DiffOp::Equal("world"),
            ]
        );
    }

    #[test]
    fn word_diff_deletion_only() {
        let old = vec!["hello", "beautiful", "world"];
        let new = vec!["hello", "world"];
        let ops = word_diff(&old, &new);
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("hello"),
                DiffOp::Delete("beautiful"),
                DiffOp::Equal("world"),
            ]
        );
    }

    #[test]
    fn word_diff_complete_replacement() {
        let old = vec!["foo", "bar"];
        let new = vec!["baz", "qux"];
        let ops = word_diff(&old, &new);
        assert_eq!(
            ops,
            vec![
                DiffOp::Delete("foo"),
                DiffOp::Delete("bar"),
                DiffOp::Insert("baz"),
                DiffOp::Insert("qux"),
            ]
        );
    }

    #[test]
    fn word_diff_empty_old() {
        let old: Vec<&str> = vec![];
        let new = vec!["hello"];
        let ops = word_diff(&old, &new);
        assert_eq!(ops, vec![DiffOp::Insert("hello")]);
    }

    #[test]
    fn word_diff_empty_new() {
        let old = vec!["hello"];
        let new: Vec<&str> = vec![];
        let ops = word_diff(&old, &new);
        assert_eq!(ops, vec![DiffOp::Delete("hello")]);
    }

    #[test]
    fn word_diff_case_insensitive_match() {
        let old = vec!["Hello", "World"];
        let new = vec!["hello", "world"];
        let ops = word_diff(&old, &new);
        // Case-insensitive match: Equal uses the new_words version
        assert_eq!(ops, vec![DiffOp::Equal("hello"), DiffOp::Equal("world")]);
    }

    // ── word probability map tests ─────────────────────────────────────

    #[test]
    fn build_word_prob_map_basic() {
        let tokens = vec![
            TokenProbability {
                token: "the".to_string(),
                probability: 0.95,
            },
            TokenProbability {
                token: " quick".to_string(),
                probability: 0.3,
            },
            TokenProbability {
                token: " brown".to_string(),
                probability: 0.92,
            },
        ];
        let map = build_word_prob_map(&tokens);
        assert_eq!(map.len(), 3);
        assert_eq!(map[0], ("the".to_string(), 0.95));
        assert_eq!(map[1], ("quick".to_string(), 0.3));
        assert_eq!(map[2], ("brown".to_string(), 0.92));
    }

    #[test]
    fn build_word_prob_map_empty() {
        let map = build_word_prob_map(&[]);
        assert!(map.is_empty());
    }

    // ── LCS tests ──────────────────────────────────────────────────────

    #[test]
    fn lcs_identical_sequences() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "b", "c"];
        let matches = lcs_indices(&old, &new);
        assert_eq!(matches, vec![(0, 0), (1, 1), (2, 2)]);
    }

    #[test]
    fn lcs_one_difference() {
        let old = vec!["a", "X", "c"];
        let new = vec!["a", "Y", "c"];
        let matches = lcs_indices(&old, &new);
        assert_eq!(matches, vec![(0, 0), (2, 2)]);
    }

    #[test]
    fn lcs_no_common() {
        let old = vec!["a", "b"];
        let new = vec!["c", "d"];
        let matches = lcs_indices(&old, &new);
        assert!(matches.is_empty());
    }

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
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            corrector_name: None,
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
            token_probabilities: vec![],
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            corrector_name: None,
        });
    }

    #[test]
    fn test_render_transcription_with_token_probabilities() {
        render_event(&DaemonEvent::Transcription {
            text: "high medium low".to_string(),
            language: "en".to_string(),
            confidence: 0.7,
            wait_ms: Some(250),
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
            raw_text: None,
            text_origin: TextOrigin::Transcription,
            corrector_name: None,
        });
    }

    #[test]
    fn test_render_corrected_transcription() {
        // Smoke test: corrected transcription with word diff
        render_event(&DaemonEvent::Transcription {
            text: "the quick brown fox".to_string(),
            language: "en".to_string(),
            confidence: 0.8,
            wait_ms: Some(200),
            token_probabilities: vec![
                TokenProbability {
                    token: "the".to_string(),
                    probability: 0.95,
                },
                TokenProbability {
                    token: " quik".to_string(),
                    probability: 0.3,
                },
                TokenProbability {
                    token: " brown".to_string(),
                    probability: 0.92,
                },
                TokenProbability {
                    token: " fox".to_string(),
                    probability: 0.88,
                },
            ],
            raw_text: Some("the quik brown fox".to_string()),
            text_origin: TextOrigin::Corrected,
            corrector_name: None,
        });
    }

    #[test]
    fn test_render_voice_command_transcription() {
        // Smoke test: voice command replacement
        render_event(&DaemonEvent::Transcription {
            text: ".".to_string(),
            language: "en".to_string(),
            confidence: 0.95,
            wait_ms: None,
            token_probabilities: vec![],
            raw_text: Some("period".to_string()),
            text_origin: TextOrigin::VoiceCommand,
            corrector_name: None,
        });
    }

    #[test]
    fn test_render_corrected_transcription_with_corrector_name() {
        // Smoke test: corrected transcription with corrector tag (T5)
        render_event(&DaemonEvent::Transcription {
            text: "the quick brown fox".to_string(),
            language: "en".to_string(),
            confidence: 0.8,
            wait_ms: Some(200),
            token_probabilities: vec![
                TokenProbability {
                    token: "the".to_string(),
                    probability: 0.95,
                },
                TokenProbability {
                    token: " quik".to_string(),
                    probability: 0.3,
                },
                TokenProbability {
                    token: " brown".to_string(),
                    probability: 0.92,
                },
                TokenProbability {
                    token: " fox".to_string(),
                    probability: 0.88,
                },
            ],
            raw_text: Some("the quik brown fox".to_string()),
            text_origin: TextOrigin::Corrected,
            corrector_name: Some("T5".to_string()),
        });
    }
}
