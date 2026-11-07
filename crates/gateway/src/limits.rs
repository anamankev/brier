//! Token-bucket rate limits (SPEC §6, §13): per account for orders, per client address for all
//! requests, and a tighter per-address limit on the password endpoints. A bucket holds up to
//! `burst` tokens and refills at a steady rate; a request that finds the bucket empty is
//! refused with 429, not queued.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::Instant;

/// Orders per second an account may sustain. Well above any human trader, low enough that one
/// misbehaving bot cannot take a large share of the core's capacity (2.7 M commands/s).
pub const ACCOUNT_ORDERS_PER_SECOND: u32 = 200;
/// Order burst an account may send at once.
pub const ACCOUNT_ORDER_BURST: u32 = 400;
/// Requests per second from one client address, for every endpoint.
pub const ADDRESS_REQUESTS_PER_SECOND: u32 = 500;
/// Request burst from one client address.
pub const ADDRESS_REQUEST_BURST: u32 = 1_000;
/// Login and registration attempts per minute from one client address. Each costs an Argon2id
/// hash (about 20 ms of CPU), and online password guessing is the main threat to sessions.
pub const ADDRESS_AUTH_PER_MINUTE: u32 = 30;
/// Login and registration burst from one client address.
pub const ADDRESS_AUTH_BURST: u32 = 10;
/// Buckets kept before idle ones are dropped, bounding memory under address churn.
const MAX_BUCKETS: usize = 100_000;

/// One token, in bucket units. A unit is 1/60,000 of a token, so a bucket refilling `r` tokens
/// per minute gains exactly `r` units per millisecond: integer arithmetic, no rounding drift.
const TOKEN: u64 = 60_000;

struct Bucket {
    units: u64,
    last: Instant,
}

/// A set of token buckets keyed by account or address.
pub struct Limiter<K> {
    per_minute: u64,
    burst: u64,
    buckets: Mutex<HashMap<K, Bucket>>,
}

impl<K: Eq + Hash + Copy> Limiter<K> {
    /// A limiter refilling `per_second` tokens per second up to `burst`.
    #[must_use]
    pub fn per_second(per_second: u32, burst: u32) -> Self {
        Self::per_minute(per_second.saturating_mul(60), burst)
    }

    /// A limiter refilling `per_minute` tokens per minute up to `burst`.
    #[must_use]
    pub fn per_minute(per_minute: u32, burst: u32) -> Self {
        Self {
            per_minute: u64::from(per_minute),
            burst: u64::from(burst).saturating_mul(TOKEN),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Take one token for `key` if one is available.
    pub fn allow(&self, key: K) -> bool {
        let now = Instant::now();
        let Ok(mut buckets) = self.buckets.lock() else {
            return false;
        };
        if buckets.len() >= MAX_BUCKETS && !buckets.contains_key(&key) {
            // Full buckets carry no information (a new bucket starts full), so drop them.
            let (per_minute, burst) = (self.per_minute, self.burst);
            buckets.retain(|_, b| refilled(b, now, per_minute, burst) < burst);
        }
        let bucket = buckets.entry(key).or_insert(Bucket {
            units: self.burst,
            last: now,
        });
        bucket.units = refilled(bucket, now, self.per_minute, self.burst);
        bucket.last = now;
        if bucket.units >= TOKEN {
            bucket.units = bucket.units.saturating_sub(TOKEN);
            true
        } else {
            false
        }
    }
}

/// The bucket's level at `now`: `per_minute` units per millisecond elapsed, capped at `burst`.
fn refilled(b: &Bucket, now: Instant, per_minute: u64, burst: u64) -> u64 {
    let elapsed_ms =
        u64::try_from(now.saturating_duration_since(b.last).as_millis()).unwrap_or(u64::MAX);
    b.units
        .saturating_add(elapsed_ms.saturating_mul(per_minute))
        .min(burst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bucket_allows_its_burst_then_refuses() {
        let limiter = Limiter::per_second(1, 3);
        assert!((0..3).all(|_| limiter.allow(7u64)));
        assert!(
            !limiter.allow(7u64),
            "the fourth request inside a second is refused"
        );
        assert!(limiter.allow(8u64), "other keys have their own bucket");
    }

    #[test]
    fn a_bucket_refills_at_its_rate() {
        let limiter = Limiter::<u64>::per_minute(60, 1); // one token per second
        let start = Instant::now();
        let mut b = Bucket {
            units: 0,
            last: start,
        };
        b.units = refilled(
            &b,
            start + std::time::Duration::from_millis(999),
            limiter.per_minute,
            limiter.burst,
        );
        assert!(b.units < TOKEN, "not yet a whole token after 999 ms");
        b.units = refilled(
            &b,
            start + std::time::Duration::from_millis(1_000),
            limiter.per_minute,
            limiter.burst,
        );
        assert_eq!(b.units, TOKEN, "exactly one token after 1 s");
    }
}
