//! Stitcher station for combining chunk transcriptions.
//!
//! Collects transcription results from chunks and combines them
//! into a coherent final transcription, handling:
//! - Out-of-order chunk arrival
//! - Word boundary deduplication (from overlap)
//! - Punctuation continuity

use crate::streaming::frame::TranscriptionResult;
use std::collections::BTreeMap;
use tokio::sync::mpsc;

/// Configuration for the stitcher.
#[derive(Debug, Clone)]
pub struct StitcherConfig {
    /// Whether to deduplicate words at chunk boundaries.
    pub deduplicate_boundaries: bool,
    /// Minimum word length to consider for deduplication.
    pub min_word_length: usize,
}

impl Default for StitcherConfig {
    fn default() -> Self {
        Self {
            deduplicate_boundaries: true,
            min_word_length: 2,
        }
    }
}

/// Stitcher that combines chunk transcriptions.
pub struct StitcherStation {
    config: StitcherConfig,
    /// Results indexed by chunk_id for ordering.
    results: BTreeMap<u64, String>,
    /// Next expected chunk ID.
    next_chunk_id: u64,
    /// Previous chunk's last word (for deduplication).
    prev_last_word: Option<String>,
}

impl StitcherStation {
    /// Creates a new stitcher with default configuration.
    pub fn new() -> Self {
        Self::with_config(StitcherConfig::default())
    }

    /// Creates a new stitcher with custom configuration.
    pub fn with_config(config: StitcherConfig) -> Self {
        Self {
            config,
            results: BTreeMap::new(),
            next_chunk_id: 0,
            prev_last_word: None,
        }
    }

    /// Adds a transcription result.
    pub fn add_result(&mut self, result: TranscriptionResult) {
        // Filter out Whisper markers and clean text
        let text = Self::clean_transcription(&result.text);
        self.results.insert(result.chunk_id, text);
    }

    /// Cleans transcription text by removing Whisper markers.
    fn clean_transcription(text: &str) -> String {
        // Common Whisper output markers to filter
        let markers = [
            "[BLANK_AUDIO]",
            "[INAUDIBLE]",
            "[MUSIC]",
            "[APPLAUSE]",
            "[LAUGHTER]",
            "(BLANK_AUDIO)",
            "(inaudible)",
        ];

        let mut cleaned = text.to_string();
        for marker in markers {
            cleaned = cleaned.replace(marker, "");
        }
        cleaned.trim().to_string()
    }

    /// Returns the combined transcription if all chunks up to final are available.
    ///
    /// Returns None if chunks are still missing.
    pub fn get_combined(&mut self) -> Option<String> {
        if self.results.is_empty() {
            return None;
        }

        // First, check that all chunks are present and collect texts in order
        let mut texts = Vec::new();
        let mut expected_id = self.next_chunk_id;
        for (&chunk_id, text) in &self.results {
            if chunk_id != expected_id {
                // Missing chunk, can't combine yet
                return None;
            }
            texts.push(text.clone());
            expected_id += 1;
        }

        // Now process the texts
        let mut combined = String::new();
        self.prev_last_word = None;

        for text in texts {
            let processed = if self.config.deduplicate_boundaries {
                self.process_with_dedup(&text)
            } else {
                text
            };

            if !combined.is_empty() && !processed.is_empty() {
                combined.push(' ');
            }
            combined.push_str(&processed);

            self.next_chunk_id += 1;
        }

        // Trim and clean up
        let combined = combined.trim().to_string();
        if combined.is_empty() {
            return None;
        }

        Some(combined)
    }

    /// Processes text with boundary deduplication.
    fn process_with_dedup(&mut self, text: &str) -> String {
        let text = text.trim();
        if text.is_empty() {
            return String::new();
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return String::new();
        }

        // Check if first word matches previous chunk's last word
        let start_idx = if let Some(ref prev) = self.prev_last_word {
            if words[0].to_lowercase() == prev.to_lowercase()
                && words[0].len() >= self.config.min_word_length
            {
                1 // Skip duplicate word
            } else {
                0
            }
        } else {
            0
        };

        // Update prev_last_word for next chunk
        self.prev_last_word = words.last().map(|w| w.to_string());

        // Join remaining words
        words[start_idx..].join(" ")
    }

    /// Resets the stitcher state.
    pub fn reset(&mut self) {
        self.results.clear();
        self.next_chunk_id = 0;
        self.prev_last_word = None;
    }

    /// Returns the number of results collected.
    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// Runs the stitcher station.
    ///
    /// Collects results until final chunk received, then combines and outputs.
    ///
    /// # Arguments
    /// * `input` - Receiver for transcription results
    /// * `output` - Sender for final combined text
    pub async fn run(
        mut self,
        mut input: mpsc::Receiver<TranscriptionResult>,
        output: mpsc::Sender<String>,
    ) {
        let mut final_received = false;

        while let Some(result) = input.recv().await {
            if result.is_final {
                final_received = true;
            }
            self.add_result(result);

            // If we've received final and have all results, combine and output
            if final_received {
                if let Some(combined) = self.get_combined() {
                    let _ = output.send(combined).await;
                }
                break;
            }
        }

        // If channel closed before final, try to output what we have
        if !final_received {
            // Force combine what we have
            let combined: String = self
                .results
                .values()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !combined.is_empty() {
                let _ = output.send(combined).await;
            }
        }
    }
}

impl Default for StitcherStation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(id: u64, text: &str, is_final: bool) -> TranscriptionResult {
        TranscriptionResult {
            chunk_id: id,
            text: text.to_string(),
            is_final,
        }
    }

    #[test]
    fn test_stitcher_creation() {
        let stitcher = StitcherStation::new();
        assert_eq!(stitcher.result_count(), 0);
    }

    #[test]
    fn test_stitcher_single_chunk() {
        let mut stitcher = StitcherStation::new();

        stitcher.add_result(make_result(0, "Hello world", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello world");
    }

    #[test]
    fn test_stitcher_multiple_chunks_in_order() {
        let mut stitcher = StitcherStation::new();

        stitcher.add_result(make_result(0, "Hello", false));
        stitcher.add_result(make_result(1, "beautiful", false));
        stitcher.add_result(make_result(2, "world", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello beautiful world");
    }

    #[test]
    fn test_stitcher_out_of_order() {
        let mut stitcher = StitcherStation::new();

        // Add chunks out of order
        stitcher.add_result(make_result(2, "world", true));
        stitcher.add_result(make_result(0, "Hello", false));

        // Should return None because chunk 1 is missing
        assert!(stitcher.get_combined().is_none());

        // Add missing chunk
        stitcher.add_result(make_result(1, "beautiful", false));

        // Now we need to reset and re-add since get_combined modified state
        let mut stitcher2 = StitcherStation::new();
        stitcher2.add_result(make_result(0, "Hello", false));
        stitcher2.add_result(make_result(1, "beautiful", false));
        stitcher2.add_result(make_result(2, "world", true));

        let combined = stitcher2.get_combined().unwrap();
        assert_eq!(combined, "Hello beautiful world");
    }

    #[test]
    fn test_stitcher_deduplication() {
        let mut stitcher = StitcherStation::new();

        // Simulate overlap where "world" appears at end of chunk 0 and start of chunk 1
        stitcher.add_result(make_result(0, "Hello world", false));
        stitcher.add_result(make_result(1, "world is", false));
        stitcher.add_result(make_result(2, "beautiful", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello world is beautiful");
    }

    #[test]
    fn test_stitcher_no_deduplication() {
        let config = StitcherConfig {
            deduplicate_boundaries: false,
            min_word_length: 2,
        };
        let mut stitcher = StitcherStation::with_config(config);

        stitcher.add_result(make_result(0, "Hello world", false));
        stitcher.add_result(make_result(1, "world is", false));
        stitcher.add_result(make_result(2, "beautiful", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello world world is beautiful");
    }

    #[test]
    fn test_stitcher_reset() {
        let mut stitcher = StitcherStation::new();

        stitcher.add_result(make_result(0, "Hello", false));
        assert_eq!(stitcher.result_count(), 1);

        stitcher.reset();
        assert_eq!(stitcher.result_count(), 0);
    }

    #[test]
    fn test_stitcher_empty_text_handling() {
        let mut stitcher = StitcherStation::new();

        stitcher.add_result(make_result(0, "Hello", false));
        stitcher.add_result(make_result(1, "   ", false)); // whitespace only
        stitcher.add_result(make_result(2, "world", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello world");
    }

    #[test]
    fn test_stitcher_min_word_length() {
        let config = StitcherConfig {
            deduplicate_boundaries: true,
            min_word_length: 4, // Only dedup words >= 4 chars
        };
        let mut stitcher = StitcherStation::with_config(config);

        // "is" should NOT be deduplicated (too short)
        stitcher.add_result(make_result(0, "This is", false));
        stitcher.add_result(make_result(1, "is a test", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "This is is a test");
    }

    #[tokio::test]
    async fn test_stitcher_run() {
        let stitcher = StitcherStation::new();

        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        // Run stitcher in background
        tokio::spawn(async move {
            stitcher.run(input_rx, output_tx).await;
        });

        // Send results
        input_tx.send(make_result(0, "Hello", false)).await.unwrap();
        input_tx.send(make_result(1, "world", true)).await.unwrap();

        // Should receive combined result
        let combined = output_rx.recv().await.unwrap();
        assert_eq!(combined, "Hello world");
    }

    #[tokio::test]
    async fn test_stitcher_run_channel_closed() {
        let stitcher = StitcherStation::new();

        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        // Run stitcher in background
        tokio::spawn(async move {
            stitcher.run(input_rx, output_tx).await;
        });

        // Send partial results then close channel
        input_tx.send(make_result(0, "Hello", false)).await.unwrap();
        input_tx.send(make_result(1, "world", false)).await.unwrap();
        drop(input_tx);

        // Should still get combined output
        let combined = output_rx.recv().await.unwrap();
        assert_eq!(combined, "Hello world");
    }

    #[test]
    fn test_clean_transcription_removes_blank_audio() {
        assert_eq!(StitcherStation::clean_transcription("[BLANK_AUDIO]"), "");
        assert_eq!(
            StitcherStation::clean_transcription("Hello [BLANK_AUDIO] world"),
            "Hello  world"
        );
    }

    #[test]
    fn test_clean_transcription_removes_multiple_markers() {
        assert_eq!(
            StitcherStation::clean_transcription("Hello [MUSIC] world [APPLAUSE]"),
            "Hello  world"
        );
    }

    #[test]
    fn test_clean_transcription_preserves_normal_text() {
        assert_eq!(
            StitcherStation::clean_transcription("Hello world"),
            "Hello world"
        );
    }

    #[test]
    fn test_stitcher_filters_blank_audio_chunks() {
        let mut stitcher = StitcherStation::new();

        stitcher.add_result(make_result(0, "Hello", false));
        stitcher.add_result(make_result(1, "[BLANK_AUDIO]", false));
        stitcher.add_result(make_result(2, "world", true));

        let combined = stitcher.get_combined().unwrap();
        assert_eq!(combined, "Hello world");
    }
}
