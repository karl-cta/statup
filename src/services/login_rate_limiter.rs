//! Sign-in rate limiter: 5 failed attempts per 15 minutes per client address.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Maximum failed login attempts before blocking.
const MAX_ATTEMPTS: u32 = 5;
/// Window duration for rate limiting.
const WINDOW: Duration = Duration::from_secs(15 * 60);

struct Entry {
    count: u32,
    first_attempt: Instant,
}

impl Entry {
    fn expired(&self) -> bool {
        self.first_attempt.elapsed() > WINDOW
    }
}

/// In-memory failed sign-in counter, keyed by client address.
#[derive(Default)]
pub struct LoginRateLimiter {
    attempts: Mutex<HashMap<IpAddr, Entry>>,
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// A panic while the map was held leaves plain counters behind, still
    /// usable, so a poisoned lock is recovered rather than propagated.
    fn attempts(&self) -> MutexGuard<'_, HashMap<IpAddr, Entry>> {
        self.attempts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Check whether the given IP is currently rate-limited.
    pub fn is_blocked(&self, ip: &IpAddr) -> bool {
        let mut map = self.attempts();
        match map.get(ip) {
            Some(entry) if entry.expired() => {
                map.remove(ip);
                false
            }
            Some(entry) => entry.count >= MAX_ATTEMPTS,
            None => false,
        }
    }

    /// Record a failed login attempt for the given IP, and forget the
    /// addresses whose window is over.
    pub fn record_failure(&self, ip: &IpAddr) {
        let mut map = self.attempts();
        map.retain(|_, entry| !entry.expired());

        let entry = map.entry(*ip).or_insert(Entry {
            count: 0,
            first_attempt: Instant::now(),
        });
        entry.count += 1;

        if entry.count == MAX_ATTEMPTS {
            tracing::warn!(
                ip = %ip,
                "Sign-in rate limit reached ({MAX_ATTEMPTS} failures in {} min)",
                WINDOW.as_secs() / 60
            );
        }
    }

    /// Clear the failure count for an IP after a successful login.
    pub fn clear(&self, ip: &IpAddr) {
        self.attempts().remove(ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().unwrap()
    }

    #[test]
    fn test_not_blocked_initially() {
        let limiter = LoginRateLimiter::new();
        assert!(!limiter.is_blocked(&ip("127.0.0.1")));
    }

    #[test]
    fn test_blocked_after_max_attempts() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip("127.0.0.1"));
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1")));
    }

    #[test]
    fn test_not_blocked_below_max() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS - 1 {
            limiter.record_failure(&ip("127.0.0.1"));
        }
        assert!(!limiter.is_blocked(&ip("127.0.0.1")));
    }

    #[test]
    fn test_clear_resets() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip("127.0.0.1"));
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1")));

        limiter.clear(&ip("127.0.0.1"));
        assert!(!limiter.is_blocked(&ip("127.0.0.1")));
    }

    #[test]
    fn test_different_ips_independent() {
        let limiter = LoginRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip("127.0.0.1"));
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1")));
        assert!(!limiter.is_blocked(&ip("192.0.2.1")));
    }

    #[test]
    fn expired_windows_are_forgotten_on_the_next_failure() {
        let Some(long_ago) = Instant::now().checked_sub(WINDOW + Duration::from_secs(1)) else {
            return;
        };
        let limiter = LoginRateLimiter::new();
        limiter.attempts().insert(
            ip("192.0.2.1"),
            Entry {
                count: MAX_ATTEMPTS,
                first_attempt: long_ago,
            },
        );

        limiter.record_failure(&ip("192.0.2.2"));

        assert_eq!(limiter.attempts().len(), 1);
        assert!(!limiter.is_blocked(&ip("192.0.2.1")));
    }
}
