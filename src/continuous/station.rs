//! Core station abstraction and runner for the continuous pipeline.

use crate::continuous::error::{ErrorReporter, StationError};
use crossbeam_channel::{Receiver, Sender};
use std::marker::PhantomData;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// A processing station in the continuous pipeline.
///
/// Each station receives input, processes it, and produces output.
/// Stations run in their own threads and are connected by channels.
pub trait Station: Send + 'static {
    /// The input type this station receives.
    type Input: Send + 'static;
    /// The output type this station produces.
    type Output: Send + 'static;

    /// Processes a single input item.
    ///
    /// Returns:
    /// - `Ok(Some(output))` - Successfully processed and produced output
    /// - `Ok(None)` - Successfully processed but no output (e.g., filtered)
    /// - `Err(StationError)` - Processing failed
    fn process(&mut self, input: Self::Input) -> Result<Option<Self::Output>, StationError>;

    /// Returns the name of this station for logging and error reporting.
    fn name(&self) -> &'static str;

    /// Called when the station is shutting down.
    ///
    /// Override this to perform cleanup operations.
    fn shutdown(&mut self) {}
}

/// Runs a station in a dedicated thread.
pub struct StationRunner<S: Station> {
    /// Handle to the spawned thread.
    handle: Option<JoinHandle<()>>,
    /// Name of the station (cached for error reporting).
    station_name: &'static str,
    /// Phantom data to mark the station type.
    _phantom: PhantomData<S>,
}

impl<S: Station> StationRunner<S> {
    /// Spawns a new station in a dedicated thread.
    ///
    /// # Arguments
    /// * `station` - The station implementation to run
    /// * `input_rx` - Channel to receive inputs from
    /// * `output_tx` - Channel to send outputs to
    /// * `error_reporter` - Reporter for handling errors
    pub fn spawn(
        mut station: S,
        input_rx: Receiver<S::Input>,
        output_tx: Sender<S::Output>,
        error_reporter: Arc<dyn ErrorReporter>,
    ) -> Self {
        let station_name = station.name();

        let handle = thread::spawn(move || {
            Self::run_station(&mut station, input_rx, output_tx, error_reporter);
        });

        Self {
            handle: Some(handle),
            station_name,
            _phantom: PhantomData,
        }
    }

    /// Main processing loop for the station.
    fn run_station(
        station: &mut S,
        input_rx: Receiver<S::Input>,
        output_tx: Sender<S::Output>,
        error_reporter: Arc<dyn ErrorReporter>,
    ) {
        let station_name = station.name();

        while let Ok(input) = input_rx.recv() {
            match station.process(input) {
                Ok(Some(output)) => {
                    // Send output to next station
                    if output_tx.send(output).is_err() {
                        // Output channel closed, shutdown
                        break;
                    }
                }
                Ok(None) => {
                    // No output produced (filtered), continue
                }
                Err(StationError::Recoverable(msg)) => {
                    // Report but continue processing
                    error_reporter.report(station_name, &StationError::Recoverable(msg));
                }
                Err(StationError::Fatal(msg)) => {
                    // Report and shutdown
                    error_reporter.report(station_name, &StationError::Fatal(msg.clone()));
                    break;
                }
            }
        }

        // Cleanup on shutdown
        station.shutdown();
    }

    /// Waits for the station thread to complete.
    ///
    /// Returns the station name for logging purposes.
    pub fn join(mut self) -> Result<(), String> {
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| format!("Station '{}' thread panicked", self.station_name))
        } else {
            Ok(())
        }
    }

    /// Returns the name of the station.
    pub fn name(&self) -> &'static str {
        self.station_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    // Mock station that doubles integers
    struct DoublerStation {
        shutdown_called: Arc<AtomicBool>,
    }

    impl Station for DoublerStation {
        type Input = i32;
        type Output = i32;

        fn process(&mut self, input: Self::Input) -> Result<Option<Self::Output>, StationError> {
            Ok(Some(input * 2))
        }

        fn name(&self) -> &'static str {
            "Doubler"
        }

        fn shutdown(&mut self) {
            self.shutdown_called.store(true, Ordering::SeqCst);
        }
    }

    // Mock station that filters even numbers
    struct FilterStation;

    impl Station for FilterStation {
        type Input = i32;
        type Output = i32;

        fn process(&mut self, input: Self::Input) -> Result<Option<Self::Output>, StationError> {
            if input % 2 == 0 {
                Ok(None) // Filter out even numbers
            } else {
                Ok(Some(input))
            }
        }

        fn name(&self) -> &'static str {
            "Filter"
        }
    }

    // Mock station that fails on certain inputs
    struct FailingStation {
        fail_on: i32,
    }

    impl Station for FailingStation {
        type Input = i32;
        type Output = i32;

        fn process(&mut self, input: Self::Input) -> Result<Option<Self::Output>, StationError> {
            if input == self.fail_on {
                Err(StationError::Recoverable(format!("Failed on {}", input)))
            } else {
                Ok(Some(input))
            }
        }

        fn name(&self) -> &'static str {
            "Failing"
        }
    }

    // Mock error reporter that collects errors
    #[derive(Default)]
    struct MockReporter {
        errors: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl ErrorReporter for MockReporter {
        fn report(&self, station: &str, error: &StationError) {
            let mut errors = self.errors.lock().unwrap();
            errors.push((station.to_string(), error.to_string()));
        }
    }

    #[test]
    fn test_station_runner_basic_processing() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        let station = DoublerStation {
            shutdown_called: shutdown_flag.clone(),
        };

        let runner = StationRunner::spawn(station, input_rx, output_tx, error_reporter);

        assert_eq!(runner.name(), "Doubler");

        // Send some inputs
        input_tx.send(1).unwrap();
        input_tx.send(2).unwrap();
        input_tx.send(3).unwrap();
        drop(input_tx); // Close channel to trigger shutdown

        // Collect outputs
        let mut outputs = Vec::new();
        while let Ok(output) = output_rx.recv() {
            outputs.push(output);
        }

        assert_eq!(outputs, vec![2, 4, 6]);

        // Wait for shutdown
        runner.join().unwrap();
        assert!(shutdown_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_station_runner_filtering() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());

        let station = FilterStation;
        let runner = StationRunner::spawn(station, input_rx, output_tx, error_reporter);

        // Send mix of even and odd numbers
        input_tx.send(1).unwrap();
        input_tx.send(2).unwrap(); // Filtered
        input_tx.send(3).unwrap();
        input_tx.send(4).unwrap(); // Filtered
        input_tx.send(5).unwrap();
        drop(input_tx);

        // Only odd numbers should pass through
        let mut outputs = Vec::new();
        while let Ok(output) = output_rx.recv() {
            outputs.push(output);
        }

        assert_eq!(outputs, vec![1, 3, 5]);
        runner.join().unwrap();
    }

    #[test]
    fn test_station_runner_error_handling() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());
        let errors = error_reporter.errors.clone();

        let station = FailingStation { fail_on: 2 };
        let runner = StationRunner::spawn(station, input_rx, output_tx, error_reporter);

        // Send inputs including the failing one
        input_tx.send(1).unwrap();
        input_tx.send(2).unwrap(); // This will fail
        input_tx.send(3).unwrap();
        drop(input_tx);

        // Collect outputs
        let mut outputs = Vec::new();
        while let Ok(output) = output_rx.recv() {
            outputs.push(output);
        }

        // All inputs should be processed except the failed one
        assert_eq!(outputs, vec![1, 3]);

        // Check error was reported
        let reported_errors = errors.lock().unwrap();
        assert_eq!(reported_errors.len(), 1);
        assert_eq!(reported_errors[0].0, "Failing");
        assert!(reported_errors[0].1.contains("Failed on 2"));

        runner.join().unwrap();
    }

    #[test]
    fn test_station_runner_graceful_shutdown() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        let station = DoublerStation {
            shutdown_called: shutdown_flag.clone(),
        };

        let runner = StationRunner::spawn(station, input_rx, output_tx, error_reporter);

        // Close input channel immediately
        drop(input_tx);

        // Station should shutdown gracefully
        runner.join().unwrap();
        assert!(shutdown_flag.load(Ordering::SeqCst));

        // Output channel should be empty
        drop(output_rx);
    }

    #[test]
    fn test_station_runner_output_channel_closed() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());
        let shutdown_flag = Arc::new(AtomicBool::new(false));

        let station = DoublerStation {
            shutdown_called: shutdown_flag.clone(),
        };

        let runner = StationRunner::spawn(station, input_rx, output_tx, error_reporter);

        // Close output channel
        drop(output_rx);

        // Send input - should trigger shutdown when trying to send output
        input_tx.send(1).unwrap();

        // Give station time to detect closed channel
        std::thread::sleep(std::time::Duration::from_millis(100));

        drop(input_tx);

        // Station should shutdown gracefully
        runner.join().unwrap();
        assert!(shutdown_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_station_runner_continues_after_multiple_errors() {
        let (input_tx, input_rx) = bounded(10);
        let (output_tx, output_rx) = bounded(10);
        let error_reporter = Arc::new(MockReporter::default());
        let errors = error_reporter.errors.clone();

        // Station that fails on even inputs
        struct EvenFailStation;
        impl Station for EvenFailStation {
            type Input = i32;
            type Output = i32;
            fn process(
                &mut self,
                input: Self::Input,
            ) -> Result<Option<Self::Output>, StationError> {
                if input % 2 == 0 {
                    Err(StationError::Recoverable(format!("Even: {}", input)))
                } else {
                    Ok(Some(input))
                }
            }
            fn name(&self) -> &'static str {
                "EvenFail"
            }
        }

        let runner = StationRunner::spawn(EvenFailStation, input_rx, output_tx, error_reporter);

        // Send 5 inputs: 1 (ok), 2 (fail), 3 (ok), 4 (fail), 5 (ok)
        for i in 1..=5 {
            input_tx.send(i).unwrap();
        }
        drop(input_tx);

        let mut outputs = Vec::new();
        while let Ok(output) = output_rx.recv() {
            outputs.push(output);
        }

        // Should have processed all 5, with 3 successes
        assert_eq!(outputs, vec![1, 3, 5]);

        // Should have reported 2 errors
        let reported = errors.lock().unwrap();
        assert_eq!(reported.len(), 2);

        runner.join().unwrap();
    }
}
