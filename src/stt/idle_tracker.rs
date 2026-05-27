//! Idle-time tracking for the Whisper transcriber.
//!
//! Tracks how long since the transcriber was last used and reports whether
//! it has been idle long enough to warrant unloading the model.

use crate::audio::vad::Clock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tracks "last used" time and reports whether the configured idle threshold
/// has been exceeded.
///
/// When `threshold` is `None`, `is_idle()` always returns `false` — idle
/// unloading is disabled entirely.
pub struct IdleTracker {
    threshold: Option<Duration>,
    last_used: Mutex<Instant>,
    clock: Arc<dyn Clock>,
}

impl IdleTracker {
    /// Creates a new tracker. Records the current clock time as the initial
    /// last-used instant so a freshly created tracker is never immediately idle.
    pub fn new(threshold: Option<Duration>, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now();
        Self {
            threshold,
            last_used: Mutex::new(now),
            clock,
        }
    }

    /// Records the current time as the most recent use, resetting the idle timer.
    #[allow(clippy::expect_used)] // Mutex poisoning means the owning thread already panicked; propagate.
    pub fn mark_used(&self) {
        let now = self.clock.now();
        *self.last_used.lock().expect("last_used mutex poisoned") = now;
    }

    /// Returns `true` iff a threshold is configured and the time since the last
    /// use meets or exceeds that threshold.
    #[allow(clippy::expect_used)] // Mutex poisoning means the owning thread already panicked; propagate.
    pub fn is_idle(&self) -> bool {
        let Some(threshold) = self.threshold else {
            return false;
        };
        let last = *self.last_used.lock().expect("last_used mutex poisoned");
        self.clock.now().duration_since(last) >= threshold
    }

    /// Returns the configured idle threshold, or `None` if idle unloading is
    /// disabled.
    pub fn threshold(&self) -> Option<Duration> {
        self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::vad::MockClock;

    fn make_tracker(threshold: Option<Duration>, clock: Arc<MockClock>) -> IdleTracker {
        IdleTracker::new(threshold, clock as Arc<dyn Clock>)
    }

    #[test]
    fn idle_tracker_with_no_threshold_is_never_idle() {
        let clock = Arc::new(MockClock::new());
        let tracker = make_tracker(None, Arc::clone(&clock));

        assert_eq!(tracker.is_idle(), false);

        clock.advance(Duration::from_secs(3600)); // 1 hour
        assert_eq!(tracker.is_idle(), false);

        clock.advance(Duration::from_secs(365 * 24 * 3600)); // ~1 year
        assert_eq!(tracker.is_idle(), false);
    }

    #[test]
    fn idle_tracker_before_threshold_is_not_idle() {
        let clock = Arc::new(MockClock::new());
        let tracker = make_tracker(Some(Duration::from_secs(5)), Arc::clone(&clock));

        clock.advance(Duration::from_secs(4));
        assert_eq!(tracker.is_idle(), false);
    }

    #[test]
    fn idle_tracker_after_threshold_is_idle() {
        let clock = Arc::new(MockClock::new());
        let tracker = make_tracker(Some(Duration::from_secs(5)), Arc::clone(&clock));

        clock.advance(Duration::from_secs(6));
        assert_eq!(tracker.is_idle(), true);
    }

    #[test]
    fn idle_tracker_mark_used_resets_clock() {
        let clock = Arc::new(MockClock::new());
        let tracker = make_tracker(Some(Duration::from_secs(5)), Arc::clone(&clock));

        clock.advance(Duration::from_secs(6));
        assert_eq!(tracker.is_idle(), true);

        tracker.mark_used();
        clock.advance(Duration::from_secs(1));
        assert_eq!(tracker.is_idle(), false);

        clock.advance(Duration::from_secs(5));
        assert_eq!(tracker.is_idle(), true);
    }

    #[test]
    fn idle_tracker_threshold_returns_configured_value() {
        let clock = Arc::new(MockClock::new());

        let with_threshold = make_tracker(Some(Duration::from_secs(30)), Arc::clone(&clock));
        assert_eq!(with_threshold.threshold(), Some(Duration::from_secs(30)));

        let without_threshold = make_tracker(None, Arc::clone(&clock));
        assert_eq!(without_threshold.threshold(), None);
    }
}
