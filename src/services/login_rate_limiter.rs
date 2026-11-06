//! Sign-in throttle. Five failures in fifteen minutes lock one account for
//! one client address, so a colleague's typos never lock the office out of
//! its own page, and a guess at one account from one address stops early.
//! A ceiling per address alone slows a walk through many accounts.
//! Checks of the current password from a signed-in session are counted per
//! account alone: only whoever holds that session can make them.
//! Requests for a password reset link, and requests to subscribe by email,
//! are counted per address, each apart from the other and from sign-in
//! failures, so asking for links in someone's name never locks them out.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::middleware::client_ip::limit_key;

/// Failures allowed per address and account before blocking.
const MAX_ATTEMPTS: u32 = 5;
/// Failures allowed per address, all accounts together.
const MAX_ATTEMPTS_PER_ADDRESS: u32 = 30;
/// Password reset link requests allowed per address, all accounts together.
const MAX_RESET_REQUESTS_PER_ADDRESS: u32 = 10;
/// Subscription requests allowed per address, all email addresses together.
const MAX_SUBSCRIBE_REQUESTS_PER_ADDRESS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    ip: IpAddr,
    account: String,
}

impl Key {
    fn new(ip: &IpAddr, email: &str) -> Self {
        Self {
            ip: limit_key(*ip),
            account: email.trim().to_lowercase(),
        }
    }
}

struct Entry {
    count: u32,
    first_attempt: Instant,
}

impl Entry {
    fn expired(&self) -> bool {
        self.first_attempt.elapsed() > WINDOW
    }
}

/// In-memory failed sign-in counter, keyed by client address and account,
/// failed current password checks, keyed by account, and reset link and
/// subscription requests, keyed by client address.
#[derive(Default)]
pub struct LoginRateLimiter {
    attempts: Mutex<HashMap<Key, Entry>>,
    password_checks: Mutex<HashMap<i64, Entry>>,
    reset_requests: Mutex<HashMap<IpAddr, Entry>>,
    subscribe_requests: Mutex<HashMap<IpAddr, Entry>>,
}

/// What a request for a reset link or a subscription meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestCheck {
    Allowed,
    /// This request is the first one refused in the window.
    JustBlocked,
    Blocked,
}

/// A panic while a map was held leaves plain counters behind, still usable,
/// so a poisoned lock is recovered rather than propagated.
fn lock<K>(map: &Mutex<HashMap<K, Entry>>) -> MutexGuard<'_, HashMap<K, Entry>> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Failures counted against this address, all accounts together.
fn address_failures(map: &HashMap<Key, Entry>, ip: &IpAddr) -> u32 {
    let block = limit_key(*ip);
    map.iter()
        .filter(|(key, _)| key.ip == block)
        .map(|(_, entry)| entry.count)
        .sum()
}

/// Counts a request from this address against `ceiling`. Only the first
/// refusal of a window says so, so that a burst of requests is journaled
/// once.
fn count_request(map: &Mutex<HashMap<IpAddr, Entry>>, ip: &IpAddr, ceiling: u32) -> RequestCheck {
    let mut map = lock(map);
    map.retain(|_, entry| !entry.expired());
    let entry = map.entry(limit_key(*ip)).or_insert(Entry {
        count: 0,
        first_attempt: Instant::now(),
    });
    if entry.count > ceiling {
        return RequestCheck::Blocked;
    }
    entry.count += 1;
    if entry.count > ceiling {
        return RequestCheck::JustBlocked;
    }
    RequestCheck::Allowed
}

impl LoginRateLimiter {
    fn attempts(&self) -> MutexGuard<'_, HashMap<Key, Entry>> {
        lock(&self.attempts)
    }

    /// Counts a password reset link request from this address, and tells
    /// whether it may go ahead.
    pub fn count_reset_request(&self, ip: &IpAddr) -> RequestCheck {
        count_request(&self.reset_requests, ip, MAX_RESET_REQUESTS_PER_ADDRESS)
    }

    /// Counts a subscription request from this address, apart from reset
    /// link requests, and tells whether it may go ahead.
    pub fn count_subscribe_request(&self, ip: &IpAddr) -> RequestCheck {
        count_request(
            &self.subscribe_requests,
            ip,
            MAX_SUBSCRIBE_REQUESTS_PER_ADDRESS,
        )
    }

    /// Whether this account has had too many wrong current passwords.
    pub fn is_password_check_blocked(&self, user_id: i64) -> bool {
        let mut map = lock(&self.password_checks);
        map.retain(|_, entry| !entry.expired());
        map.get(&user_id)
            .is_some_and(|entry| entry.count >= MAX_ATTEMPTS)
    }

    /// Record a wrong current password for this account. True when this one
    /// blocks the account.
    pub fn record_password_check_failure(&self, user_id: i64) -> bool {
        let mut map = lock(&self.password_checks);
        map.retain(|_, entry| !entry.expired());
        let entry = map.entry(user_id).or_insert(Entry {
            count: 0,
            first_attempt: Instant::now(),
        });
        entry.count += 1;
        entry.count == MAX_ATTEMPTS
    }

    /// Forget the wrong current passwords of this account.
    pub fn clear_password_checks(&self, user_id: i64) {
        lock(&self.password_checks).remove(&user_id);
    }

    /// Whether this address is blocked from trying this account.
    pub fn is_blocked(&self, ip: &IpAddr, email: &str) -> bool {
        let mut map = self.attempts();
        map.retain(|_, entry| !entry.expired());
        let account = map
            .get(&Key::new(ip, email))
            .is_some_and(|entry| entry.count >= MAX_ATTEMPTS);
        account || address_failures(&map, ip) >= MAX_ATTEMPTS_PER_ADDRESS
    }

    /// Record a failed sign-in for this address and account, and forget the
    /// pairs whose window is over. True when this failure blocks the account
    /// or the address.
    pub fn record_failure(&self, ip: &IpAddr, email: &str) -> bool {
        let mut map = self.attempts();
        map.retain(|_, entry| !entry.expired());

        let entry = map.entry(Key::new(ip, email)).or_insert(Entry {
            count: 0,
            first_attempt: Instant::now(),
        });
        entry.count += 1;
        let account_blocked = entry.count == MAX_ATTEMPTS;
        account_blocked || address_failures(&map, ip) == MAX_ATTEMPTS_PER_ADDRESS
    }

    /// Clear the failure count for this address and account after a
    /// successful sign-in.
    pub fn clear(&self, ip: &IpAddr, email: &str) {
        self.attempts().remove(&Key::new(ip, email));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().unwrap()
    }

    const ALICE: &str = "alice@example.org";
    const BOB: &str = "bob@example.org";

    #[test]
    fn reset_requests_stop_at_the_address_ceiling_and_lock_no_one_out() {
        let limiter = LoginRateLimiter::default();
        for _ in 0..MAX_RESET_REQUESTS_PER_ADDRESS {
            assert_eq!(
                limiter.count_reset_request(&ip("127.0.0.1")),
                RequestCheck::Allowed
            );
        }
        assert_eq!(
            limiter.count_reset_request(&ip("127.0.0.1")),
            RequestCheck::JustBlocked
        );
        assert_eq!(
            limiter.count_reset_request(&ip("127.0.0.1")),
            RequestCheck::Blocked
        );
        assert_eq!(
            limiter.count_reset_request(&ip("192.0.2.1")),
            RequestCheck::Allowed
        );
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), ALICE));
    }

    #[test]
    fn reset_and_subscription_requests_have_their_own_ceilings() {
        let limiter = LoginRateLimiter::default();
        let address = ip("127.0.0.1");
        for _ in 0..MAX_SUBSCRIBE_REQUESTS_PER_ADDRESS {
            assert_eq!(
                limiter.count_subscribe_request(&address),
                RequestCheck::Allowed
            );
        }
        assert_eq!(
            limiter.count_subscribe_request(&address),
            RequestCheck::JustBlocked
        );
        assert_eq!(
            limiter.count_subscribe_request(&address),
            RequestCheck::Blocked
        );
        assert_eq!(limiter.count_reset_request(&address), RequestCheck::Allowed);

        let other = ip("192.0.2.1");
        for _ in 0..=MAX_RESET_REQUESTS_PER_ADDRESS {
            limiter.count_reset_request(&other);
        }
        assert_eq!(limiter.count_reset_request(&other), RequestCheck::Blocked);
        assert_eq!(
            limiter.count_subscribe_request(&other),
            RequestCheck::Allowed
        );
    }

    #[test]
    fn only_the_failure_that_blocks_says_so() {
        let limiter = LoginRateLimiter::default();
        let blocking: Vec<bool> = (0..=MAX_ATTEMPTS)
            .map(|_| limiter.record_failure(&ip("127.0.0.1"), ALICE))
            .collect();
        assert_eq!(blocking, [false, false, false, false, true, false]);

        let walking = LoginRateLimiter::default();
        let blocking = (0..MAX_ATTEMPTS_PER_ADDRESS)
            .filter(|n| walking.record_failure(&ip("127.0.0.1"), &format!("user{n}@example.org")))
            .count();
        assert_eq!(blocking, 1, "the address ceiling is reached once");

        let checks = LoginRateLimiter::default();
        let blocking = (0..=MAX_ATTEMPTS)
            .filter(|_| checks.record_password_check_failure(1))
            .count();
        assert_eq!(blocking, 1);
    }

    #[test]
    fn not_blocked_initially() {
        let limiter = LoginRateLimiter::default();
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), ALICE));
    }

    #[test]
    fn blocked_after_max_attempts_on_one_account() {
        let limiter = LoginRateLimiter::default();
        for _ in 0..MAX_ATTEMPTS - 1 {
            limiter.record_failure(&ip("127.0.0.1"), ALICE);
        }
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), ALICE));
        limiter.record_failure(&ip("127.0.0.1"), " Alice@Example.org ");
        assert!(limiter.is_blocked(&ip("127.0.0.1"), ALICE));
    }

    #[test]
    fn other_accounts_and_addresses_stay_open() {
        let limiter = LoginRateLimiter::default();
        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip("127.0.0.1"), ALICE);
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1"), ALICE));
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), BOB));
        assert!(!limiter.is_blocked(&ip("192.0.2.1"), ALICE));
    }

    #[test]
    fn an_address_walking_through_accounts_is_stopped() {
        let limiter = LoginRateLimiter::default();
        for n in 0..MAX_ATTEMPTS_PER_ADDRESS {
            limiter.record_failure(&ip("127.0.0.1"), &format!("user{n}@example.org"));
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1"), "someone-else@example.org"));
        assert!(!limiter.is_blocked(&ip("192.0.2.1"), "someone-else@example.org"));
    }

    #[test]
    fn a_new_address_in_the_same_ipv6_block_is_still_blocked() {
        let limiter = LoginRateLimiter::default();
        for n in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip(&format!("2001:db8:0:1::{n}")), ALICE);
        }
        assert!(limiter.is_blocked(&ip("2001:db8:0:1::ffff"), ALICE));
        assert!(!limiter.is_blocked(&ip("2001:db8:0:2::1"), ALICE));
    }

    #[test]
    fn clear_resets_the_pair() {
        let limiter = LoginRateLimiter::default();
        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure(&ip("127.0.0.1"), ALICE);
        }
        assert!(limiter.is_blocked(&ip("127.0.0.1"), ALICE));

        limiter.clear(&ip("127.0.0.1"), ALICE);
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), ALICE));
    }

    #[test]
    fn current_password_checks_are_counted_per_account() {
        let limiter = LoginRateLimiter::default();
        for _ in 0..MAX_ATTEMPTS {
            assert!(!limiter.is_password_check_blocked(1));
            limiter.record_password_check_failure(1);
        }
        assert!(limiter.is_password_check_blocked(1));
        assert!(!limiter.is_password_check_blocked(2));
        assert!(!limiter.is_blocked(&ip("127.0.0.1"), ALICE));

        limiter.clear_password_checks(1);
        assert!(!limiter.is_password_check_blocked(1));
    }

    #[test]
    fn expired_windows_are_forgotten() {
        let Some(long_ago) = Instant::now().checked_sub(WINDOW + Duration::from_secs(1)) else {
            return;
        };
        let limiter = LoginRateLimiter::default();
        limiter.attempts().insert(
            Key::new(&ip("192.0.2.1"), ALICE),
            Entry {
                count: MAX_ATTEMPTS,
                first_attempt: long_ago,
            },
        );
        assert!(!limiter.is_blocked(&ip("192.0.2.1"), ALICE));
        assert!(limiter.attempts().is_empty());
    }
}
