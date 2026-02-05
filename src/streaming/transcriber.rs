//! Async transcriber station.
//!
//! Processes audio chunks asynchronously without blocking the recording pipeline.
//! Uses tokio::spawn_blocking to run Whisper inference on a thread pool.

use crate::error::{Result, VoicshError};
use crate::streaming::frame::{ChunkData, TranscriptionResult};
use crate::stt::transcriber::Transcriber;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Transcriber station that processes chunks asynchronously.
pub struct TranscriberStation<T: Transcriber> {
    transcriber: Arc<T>,
}

impl<T: Transcriber + Send + Sync + 'static> TranscriberStation<T> {
    /// Creates a new transcriber station wrapping the given transcriber.
    pub fn new(transcriber: T) -> Self {
        Self {
            transcriber: Arc::new(transcriber),
        }
    }

    /// Creates a new transcriber station from an Arc.
    pub fn from_arc(transcriber: Arc<T>) -> Self {
        Self { transcriber }
    }

    /// Transcribes a single chunk.
    pub fn transcribe(&self, chunk: &ChunkData) -> Result<TranscriptionResult> {
        let output = self.transcriber.transcribe(&chunk.samples)?;

        Ok(TranscriptionResult {
            chunk_id: chunk.chunk_id,
            text: output.text,
            is_final: chunk.is_final,
        })
    }

    /// Transcribes a chunk asynchronously using spawn_blocking.
    pub async fn transcribe_async(&self, chunk: ChunkData) -> Result<TranscriptionResult> {
        let transcriber = self.transcriber.clone();
        let chunk_id = chunk.chunk_id;
        let is_final = chunk.is_final;

        // Run blocking transcription on tokio's blocking thread pool
        let output = tokio::task::spawn_blocking(move || transcriber.transcribe(&chunk.samples))
            .await
            .map_err(|e| VoicshError::Transcription {
                message: format!("Transcription task panicked: {}", e),
            })??;

        Ok(TranscriptionResult {
            chunk_id,
            text: output.text,
            is_final,
        })
    }

    /// Runs the transcriber station.
    ///
    /// Receives chunks, transcribes them asynchronously, and sends results.
    /// Multiple chunks can be processed concurrently.
    ///
    /// # Arguments
    /// * `input` - Receiver for chunk data
    /// * `output` - Sender for transcription results
    /// * `max_concurrent` - Maximum number of concurrent transcriptions (default: 2)
    pub async fn run(
        self,
        mut input: mpsc::Receiver<ChunkData>,
        output: mpsc::Sender<TranscriptionResult>,
        max_concurrent: usize,
    ) {
        use tokio::sync::Semaphore;

        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let transcriber = self.transcriber.clone();

        while let Some(chunk) = input.recv().await {
            let is_final = chunk.is_final;
            let permit = semaphore.clone().acquire_owned().await;
            let transcriber = transcriber.clone();
            let output = output.clone();

            // Spawn a task for this chunk
            tokio::spawn(async move {
                let _permit = permit; // Hold permit until done

                let chunk_id = chunk.chunk_id;
                let result =
                    tokio::task::spawn_blocking(move || transcriber.transcribe(&chunk.samples))
                        .await;

                match result {
                    Ok(Ok(transcription)) => {
                        let frame = TranscriptionResult {
                            chunk_id,
                            text: transcription.text,
                            is_final,
                        };
                        let _ = output.send(frame).await;
                    }
                    Ok(Err(e)) => {
                        eprintln!("Transcription error for chunk {}: {}", chunk_id, e);
                    }
                    Err(e) => {
                        eprintln!("Transcription task panicked for chunk {}: {}", chunk_id, e);
                    }
                }
            });

            // If this was the final chunk, we're done receiving
            if is_final {
                break;
            }
        }

        // Wait for all pending transcriptions by acquiring all permits
        // This ensures we don't exit before all work is done
        let _ = semaphore.acquire_many(max_concurrent as u32).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::transcriber::{MockTranscriber, Transcriber, TranscriptionResult as SttResult};

    fn make_chunk(id: u64, is_final: bool) -> ChunkData {
        ChunkData {
            chunk_id: id,
            start_sequence: 0,
            end_sequence: 10,
            samples: vec![1000i16; 1600],
            flushed_early: false,
            is_final,
        }
    }

    #[test]
    fn test_transcriber_station_creation() {
        let mock = MockTranscriber::new("mock-model").with_response("hello world");
        let _station = TranscriberStation::new(mock);
    }

    #[test]
    fn test_transcriber_station_transcribe_sync() {
        let mock = MockTranscriber::new("mock-model").with_response("test transcription");
        let station = TranscriberStation::new(mock);

        let chunk = make_chunk(42, false);
        let result = station.transcribe(&chunk).unwrap();

        assert_eq!(result.chunk_id, 42);
        assert_eq!(result.text, "test transcription");
        assert!(!result.is_final);
    }

    #[tokio::test]
    async fn test_transcriber_station_transcribe_async() {
        let mock = MockTranscriber::new("mock-model").with_response("async result");
        let station = TranscriberStation::new(mock);

        let chunk = make_chunk(5, true);
        let result = station.transcribe_async(chunk).await.unwrap();

        assert_eq!(result.chunk_id, 5);
        assert_eq!(result.text, "async result");
        assert!(result.is_final);
    }

    #[tokio::test]
    async fn test_transcriber_station_run() {
        let mock = MockTranscriber::new("mock-model").with_response("chunk result");
        let station = TranscriberStation::new(mock);

        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        // Run station in background
        tokio::spawn(async move {
            station.run(input_rx, output_tx, 2).await;
        });

        // Send a chunk
        input_tx.send(make_chunk(0, false)).await.unwrap();

        // Should receive result
        let result = output_rx.recv().await.unwrap();
        assert_eq!(result.chunk_id, 0);
        assert_eq!(result.text, "chunk result");

        // Send final chunk
        input_tx.send(make_chunk(1, true)).await.unwrap();

        // Should receive final result
        let result = output_rx.recv().await.unwrap();
        assert_eq!(result.chunk_id, 1);
        assert!(result.is_final);

        // Drop to allow completion
        drop(input_tx);
    }

    #[tokio::test]
    async fn test_transcriber_station_concurrent() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::Duration;

        // Transcriber that tracks concurrent executions
        struct SlowTranscriber {
            concurrent: Arc<AtomicU32>,
            max_concurrent: Arc<AtomicU32>,
        }

        impl Transcriber for SlowTranscriber {
            fn transcribe(&self, _samples: &[i16]) -> Result<SttResult> {
                let current = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_concurrent.fetch_max(current, Ordering::SeqCst);

                // Simulate slow transcription
                std::thread::sleep(Duration::from_millis(50));

                self.concurrent.fetch_sub(1, Ordering::SeqCst);
                Ok(SttResult::from_text("result".to_string()))
            }

            fn model_name(&self) -> &str {
                "slow-mock"
            }

            fn is_ready(&self) -> bool {
                true
            }
        }

        let concurrent = Arc::new(AtomicU32::new(0));
        let max_concurrent = Arc::new(AtomicU32::new(0));

        let transcriber = SlowTranscriber {
            concurrent: concurrent.clone(),
            max_concurrent: max_concurrent.clone(),
        };
        let station = TranscriberStation::new(transcriber);

        let (input_tx, input_rx) = mpsc::channel(10);
        let (output_tx, mut output_rx) = mpsc::channel(10);

        // Run with max_concurrent = 2
        tokio::spawn(async move {
            station.run(input_rx, output_tx, 2).await;
        });

        // Send multiple chunks quickly
        for i in 0..4 {
            input_tx.send(make_chunk(i, i == 3)).await.unwrap();
        }

        // Collect all results
        let mut results = Vec::new();
        while let Some(result) = output_rx.recv().await {
            results.push(result);
            if results.len() == 4 {
                break;
            }
        }

        // Verify max concurrency was limited to 2
        assert!(
            max_concurrent.load(Ordering::SeqCst) <= 2,
            "Max concurrent was {} (should be <= 2)",
            max_concurrent.load(Ordering::SeqCst)
        );
    }
}
