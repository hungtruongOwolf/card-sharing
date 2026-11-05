//! Time source. Domain code asks a [`Clock`] for "now" instead of calling the system clock, so
//! tests can control expiry, grace windows and cooldowns deterministically.

use std::sync::{Arc, Mutex};

use time::{Duration, OffsetDateTime};

pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> OffsetDateTime;
}

/// Shared clock handle used in services and application state.
pub type SharedClock = Arc<dyn Clock>;

/// The real wall clock (UTC).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// A clock that only moves when told to. For tests.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<OffsetDateTime>,
}

impl ManualClock {
    pub fn new(start: OffsetDateTime) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Starts at the current wall-clock time (truncated to whole seconds).
    pub fn starting_now() -> Self {
        let now = OffsetDateTime::now_utc();
        Self::new(now.replace_nanosecond(0).unwrap_or(now))
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self
            .now
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *now += by;
    }

    pub fn set(&self, to: OffsetDateTime) {
        let mut now = self
            .now
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *now = to;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> OffsetDateTime {
        *self
            .now
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_advanced() {
        let start = OffsetDateTime::UNIX_EPOCH;
        let clock = ManualClock::new(start);
        assert_eq!(clock.now(), start);
        clock.advance(Duration::seconds(30));
        assert_eq!(clock.now(), start + Duration::seconds(30));
    }

    #[test]
    fn system_clock_is_utc() {
        assert!(SystemClock.now().offset().is_utc());
    }
}
