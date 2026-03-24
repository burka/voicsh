//! Error types and reporting for continuous pipeline stations.

use std::fmt;

/// Errors that can occur during station processing.
///
/// Carries a boxed `std::error::Error` to preserve the full error chain
/// rather than collapsing context into a `String`.
pub enum StationError {
    /// Recoverable error that allows the station to continue processing.
    Recoverable(Box<dyn std::error::Error + Send + Sync>),
    /// Fatal error that requires the station to shut down.
    Fatal(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Debug for StationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StationError::Recoverable(e) => write!(f, "StationError::Recoverable({:?})", e),
            StationError::Fatal(e) => write!(f, "StationError::Fatal({:?})", e),
        }
    }
}

impl fmt::Display for StationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StationError::Recoverable(e) => write!(f, "Recoverable error: {}", e),
            StationError::Fatal(e) => write!(f, "Fatal error: {}", e),
        }
    }
}

impl std::error::Error for StationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StationError::Recoverable(e) => Some(e.as_ref()),
            StationError::Fatal(e) => Some(e.as_ref()),
        }
    }
}

impl StationError {
    /// Creates a `Recoverable` error from any string message.
    pub fn recoverable(msg: impl Into<String>) -> Self {
        StationError::Recoverable(msg.into().into())
    }

    /// Creates a `Fatal` error from any string message.
    pub fn fatal(msg: impl Into<String>) -> Self {
        StationError::Fatal(msg.into().into())
    }
}

impl From<crate::error::VoicshError> for StationError {
    fn from(e: crate::error::VoicshError) -> Self {
        StationError::Recoverable(Box::new(e))
    }
}

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
        let recoverable = StationError::recoverable("temporary failure");
        assert_eq!(
            recoverable.to_string(),
            "Recoverable error: temporary failure"
        );

        let fatal = StationError::fatal("critical failure");
        assert_eq!(fatal.to_string(), "Fatal error: critical failure");
    }

    #[test]
    fn test_log_reporter() {
        let reporter = LogReporter;

        // Test Recoverable variant - ensures it doesn't panic (no capturable stderr in tests)
        let recoverable = StationError::recoverable("test error");
        reporter.report("TestStation", &recoverable);

        // Test Fatal variant - ensures it doesn't panic (no capturable stderr in tests)
        let fatal = StationError::fatal("critical error");
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
        let recoverable = StationError::recoverable("temp failure");
        reporter.report("AudioStation", &recoverable);

        // Test Fatal error
        let fatal = StationError::fatal("shutdown required");
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
        let error = StationError::recoverable("test");
        // Verify it can be used as std::error::Error trait object
        let _: &dyn std::error::Error = &error;

        let fatal = StationError::fatal("test");
        let _: &dyn std::error::Error = &fatal;
    }

    #[test]
    fn test_station_error_source_chain() {
        use crate::error::VoicshError;

        let inner = VoicshError::Other("inner cause".into());
        let station_err = StationError::from(inner);

        // Verify the error source is accessible
        let source = std::error::Error::source(&station_err).unwrap();
        assert!(source.to_string().contains("inner cause"));
    }

    #[test]
    fn test_station_error_display_delegates_to_inner() {
        let recoverable = StationError::recoverable("detailed message");
        assert_eq!(
            recoverable.to_string(),
            "Recoverable error: detailed message"
        );

        let fatal = StationError::fatal("shutdown reason");
        assert_eq!(fatal.to_string(), "Fatal error: shutdown reason");
    }

    #[test]
    fn test_eprintln_clear() {
        // Verify it doesn't panic - it writes to stderr so we can't capture output
        eprintln_clear("test message");
        eprintln_clear("");
        eprintln_clear("longer message with special chars: !@#$%");
    }
}
