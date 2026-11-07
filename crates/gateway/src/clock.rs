//! The gateway's clock, in Unix seconds. Market schedules, proposals, and the dispute window are
//! all judged by it; tests replace the system clock with one they set by hand.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970-01-01T00:00:00Z.
pub type Unix = i64;

/// The system clock, or a manual one for tests.
#[derive(Debug, Clone, Default)]
pub struct Clock(Option<Arc<AtomicI64>>);

impl Clock {
    /// The system clock.
    #[must_use]
    pub const fn system() -> Self {
        Self(None)
    }

    /// A clock that reads `start` until [`Clock::set`] changes it.
    #[must_use]
    pub fn manual(start: Unix) -> Self {
        Self(Some(Arc::new(AtomicI64::new(start))))
    }

    /// The current time.
    #[must_use]
    pub fn now(&self) -> Unix {
        self.0.as_ref().map_or_else(
            || {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
            },
            |t| t.load(Ordering::Acquire),
        )
    }

    /// Set a manual clock; a system clock ignores this.
    pub fn set(&self, t: Unix) {
        if let Some(c) = &self.0 {
            c.store(t, Ordering::Release);
        }
    }
}
