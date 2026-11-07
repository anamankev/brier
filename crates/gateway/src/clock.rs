//! The gateway's clock, in Unix seconds. Market schedules, proposals, and the dispute window are
//! all judged by it.
//!
//! In-process tests use a manual clock ([`Clock::manual`]). The end-to-end exit test drives a
//! real gateway process through months of schedule in seconds with a file clock
//! (`Clock::from_file`), which exists only in builds with the `test-clock` feature; a normal
//! build has no way to move its clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970-01-01T00:00:00Z.
pub type Unix = i64;

#[derive(Debug, Clone, Default)]
enum Source {
    #[default]
    System,
    Manual(Arc<AtomicI64>),
    #[cfg(feature = "test-clock")]
    File(std::path::PathBuf),
}

/// The system clock, or a test clock.
#[derive(Debug, Clone, Default)]
pub struct Clock(Source);

impl Clock {
    /// The system clock.
    #[must_use]
    pub const fn system() -> Self {
        Self(Source::System)
    }

    /// A clock that reads `start` until [`Clock::set`] changes it.
    #[must_use]
    pub fn manual(start: Unix) -> Self {
        Self(Source::Manual(Arc::new(AtomicI64::new(start))))
    }

    /// A clock that reads Unix seconds from `path` on every call (`test-clock` builds only).
    /// An unreadable file reads as 0, before every schedule, so nothing happens early.
    #[cfg(feature = "test-clock")]
    #[must_use]
    pub const fn from_file(path: std::path::PathBuf) -> Self {
        Self(Source::File(path))
    }

    /// The current time.
    #[must_use]
    pub fn now(&self) -> Unix {
        match &self.0 {
            Source::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            Source::Manual(t) => t.load(Ordering::Acquire),
            #[cfg(feature = "test-clock")]
            Source::File(path) => std::fs::read_to_string(path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0),
        }
    }

    /// Set a manual clock; any other clock ignores this.
    pub fn set(&self, t: Unix) {
        if let Source::Manual(c) = &self.0 {
            c.store(t, Ordering::Release);
        }
    }
}
