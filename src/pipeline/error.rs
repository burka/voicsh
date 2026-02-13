//! Error types and reporting for continuous pipeline stations.

use std::fmt;

/// Errors that can occur during station processing.
#[derive(Debug, Clone)]
pub enum StationError {
    /// Recoverable error that allows the station to continue processing.
    Recoverable(String),
    /// Fatal error that requires the station to shut down.
    Fatal(String),
}

impl fmt::Display for StationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StationError::Recoverable(msg) => write!(f, "Recoverable error: {}", msg),
            StationError::Fatal(msg) => write!(f, "Fatal error: {}", msg),
        }
    }
}

impl std::error::Error for StationError {}

/// Trait for reporting station errors.
pub trait ErrorReporter: Send + Sync {
    /// Reports an error from a station.
    fn report(&self, station: &str, error: &StationError);
}

/// Simple error reporter that logs to stderr.
#[derive(Debug, Clone, Copy, Default)]
pub struct LogReporter;

impl ErrorReporter for LogReporter {
    fn report(&self, station: &str, error: &StationError) {
        eprintln!("[{}] {}", station, error);
    }
}

/// Print a message to stderr, clearing any active level meter line first.
pub fn eprintln_clear(msg: &str) {
    eprint!("\r\x1b[2K");
    eprintln!("{}", msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_station_error_display() {
        let recoverable = StationError::Recoverable("temporary failure".to_string());
        assert_eq!(
            recoverable.to_string(),
            "Recoverable error: temporary failure"
        );

        let fatal = StationError::Fatal("critical failure".to_string());
        assert_eq!(fatal.to_string(), "Fatal error: critical failure");
    }

    #[test]
    fn test_log_reporter() {
        let reporter = LogReporter;

        // Test Recoverable variant - ensure it doesn't panic
        let recoverable = StationError::Recoverable("test error".to_string());
        reporter.report("TestStation", &recoverable);

        // Test Fatal variant - ensure it doesn't panic
        let fatal = StationError::Fatal("critical error".to_string());
        reporter.report("TestStation", &fatal);
    }

    #[test]
    fn test_recording_reporter() {
        use std::sync::Mutex;

        /// Test-only reporter that records calls for verification.
        struct RecordingReporter {
            calls: Mutex<Vec<(String, String)>>,
        }

        impl ErrorReporter for RecordingReporter {
            fn report(&self, station: &str, error: &StationError) {
                self.calls
                    .lock()
                    .unwrap()
                    .push((station.to_string(), error.to_string()));
            }
        }

        let reporter = RecordingReporter {
            calls: Mutex::new(Vec::new()),
        };

        // Test Recoverable error
        let recoverable = StationError::Recoverable("temp failure".to_string());
        reporter.report("AudioStation", &recoverable);

        // Test Fatal error
        let fatal = StationError::Fatal("shutdown required".to_string());
        reporter.report("TranscriberStation", &fatal);

        // Verify both calls were recorded correctly
        let calls = reporter.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);

        assert_eq!(calls[0].0, "AudioStation");
        assert_eq!(calls[0].1, "Recoverable error: temp failure");

        assert_eq!(calls[1].0, "TranscriberStation");
        assert_eq!(calls[1].1, "Fatal error: shutdown required");
    }

    #[test]
    fn test_station_error_is_std_error() {
        let error = StationError::Recoverable("test".to_string());
        // Verify it can be used as std::error::Error trait object
        let _: &dyn std::error::Error = &error;

        let fatal = StationError::Fatal("test".to_string());
        let _: &dyn std::error::Error = &fatal;
    }

    #[test]
    fn test_station_error_clone() {
        // Test Clone for Recoverable variant
        let recoverable = StationError::Recoverable("original".to_string());
        let cloned = recoverable.clone();
        assert_eq!(recoverable.to_string(), cloned.to_string());

        // Test Clone for Fatal variant
        let fatal = StationError::Fatal("critical".to_string());
        let cloned = fatal.clone();
        assert_eq!(fatal.to_string(), cloned.to_string());
    }

    #[test]
    fn test_eprintln_clear() {
        // Verify it doesn't panic - it writes to stderr so we can't capture output
        eprintln_clear("test message");
        eprintln_clear("");
        eprintln_clear("longer message with special chars: !@#$%");
    }
}
