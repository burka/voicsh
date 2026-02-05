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
    eprint!("\r{:60}\r", "");
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
        let error = StationError::Recoverable("test error".to_string());
        // Just ensure it doesn't panic
        reporter.report("TestStation", &error);
    }
}
