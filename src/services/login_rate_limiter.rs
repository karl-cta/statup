//! Login rate limiter, max 5 failed attempts per 15 minutes per IP.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Maximum failed login attempts before blocking.
const MAX_ATTEMPTS: u32 = 5;
/// Window duration for rate limiting.
const WINDOW: Duration = Duration::from_secs(15 * 60);

struct Entry {
    count: u32,
    first_attempt: Instant,
}

/// In-memory rate limiter for login attempts, keyed by IP address.
pub struct LoginRateLimiter {
    attempts: Mutex<HashMap<IpAddr, Entry>>,
}

#[allow(clippy::missing_panics_doc)]
impl LoginRateLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Check whether the given IP is currently rate-limited.
    pub fn is_blocked(&self, ip: &IpAddr) -> bool {
        let mut map = self.attempts.lock().expect("rate limiter lock poisoned");

        if let Some(entry) = map.get(ip) {
            if entry.first_attempt.elapsed() > WINDOW {
                // Window expired, remove entry
                map.remove(ip);
                false
            } else {
                entry.count >= MAX_ATTEMPTS
            }
        } else {
            false
        }
    }

    /// Record a failed login attempt for the given IP.
    pub fn record_failure(&self, ip: &IpAddr) {
        let mut map = self.attempts.lock().expect("rate limiter lock poisoned");

        let entry = map.entry(*ip).or_insert(Entry {
            count: 0,
            first_attempt: Instant::now(),
        });

        // Reset window if expired
        if entry.first_attempt.elapsed() > WINDOW {
            entry.count = 0;
            entry.first_attempt = Instant::now();
        }

        entry.count += 1;

        if entry.count >= MAX_ATTEMPTS {
            tracing::warn!(
                ip = %ip,
                "Login rate limit reached ({MAX_ATTEMPTS} failures in {} min)",
                WINDOW.as_secs() / 60
            );
        }
    }

    /// Clear the failure count for an IP after a successful login.
    pub fn clear(&self, ip: &IpAddr) {
        self.attempts
            .lock()
            .expect("rate limiter lock poisoned")
            .remove(ip);
    }
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_blocked_initially() {
        let limiter = LoginRateLimiter::new();
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(!limiter.is_blocked(&ip));
    }

    #[test]
    fn test_blocked_after_max_attempts() {
        let limiter = LoginRateLimiter::new();
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip);
        }

        assert!(limiter.is_blocked(&ip));
    }

    #[test]
    fn test_not_blocked_below_max() {
        let limiter = LoginRateLimiter::new();
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        for _ in 0..MAX_ATTEMPTS - 1 {
            limiter.record_failure(&ip);
        }

        assert!(!limiter.is_blocked(&ip));
    }

    #[test]
    fn test_clear_resets() {
        let limiter = LoginRateLimiter::new();
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip);
        }
        assert!(limiter.is_blocked(&ip));

        limiter.clear(&ip);
        assert!(!limiter.is_blocked(&ip));
    }

    #[test]
    fn test_different_ips_independent() {
        let limiter = LoginRateLimiter::new();
        let ip1: IpAddr = "127.0.0.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.1".parse().unwrap();

        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip1);
        }

        assert!(limiter.is_blocked(&ip1));
        assert!(!limiter.is_blocked(&ip2));
    }
}
