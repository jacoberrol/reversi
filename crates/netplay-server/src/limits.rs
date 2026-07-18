//! Server-side rate limiting: drop-and-log guards applied before the lobby.
//!
//! Defense in depth beyond auth — an authorized-but-buggy/compromised client
//! still can't spray the relay. Every threshold is a tunable `const` here.
//! Layers: a handshake timeout (in the connection handler), per-IP concurrency
//! and new-connection rate ([`IpLimiter`]), a per-connection inbound message
//! bucket ([`message_bucket`]), and a lobby player cap (in the lobby actor).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A connection must send a valid `Hello` within this long or be dropped.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Max concurrent connections from a single IP.
pub const MAX_CONCURRENT_PER_IP: u32 = 8;
/// New-connection rate per IP: a bucket of this size, refilled `NEW_CONN_PER_SEC`.
const NEW_CONN_BURST: f64 = 10.0;
const NEW_CONN_PER_SEC: f64 = 1.0;
/// Per-connection inbound message rate: a bucket of this size, refilled `MSG_PER_SEC`.
const MSG_BURST: f64 = 40.0;
const MSG_PER_SEC: f64 = 20.0;
/// Max simultaneous players in the lobby (bounds presence-broadcast fan-out).
pub const MAX_LOBBY_PLAYERS: usize = 200;

/// A simple token bucket: `capacity` tokens, refilled at `refill_per_sec`.
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_sec: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_per_sec,
            last: Instant::now(),
        }
    }

    /// Take one token; `false` if empty (over the limit).
    pub fn try_take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// A fresh per-connection inbound-message limiter.
pub fn message_bucket() -> TokenBucket {
    TokenBucket::new(MSG_BURST, MSG_PER_SEC)
}

/// Per-IP limits: concurrent connections plus new-connection rate. Shared across
/// connection tasks behind a `Mutex` (locked only briefly, never across await).
pub struct IpLimiter {
    concurrent: HashMap<IpAddr, u32>,
    rate: HashMap<IpAddr, TokenBucket>,
    max_concurrent: u32,
    burst: f64,
    per_sec: f64,
}

impl IpLimiter {
    /// The production limiter (uses the module constants).
    pub fn new() -> Self {
        Self::with_limits(MAX_CONCURRENT_PER_IP, NEW_CONN_BURST, NEW_CONN_PER_SEC)
    }

    pub fn with_limits(max_concurrent: u32, burst: f64, per_sec: f64) -> Self {
        Self {
            concurrent: HashMap::new(),
            rate: HashMap::new(),
            max_concurrent,
            burst,
            per_sec,
        }
    }

    /// Try to admit a new connection from `ip`. On success the caller must hold
    /// an [`IpGuard`] so the slot is released when the connection ends.
    pub fn admit(&mut self, ip: IpAddr) -> bool {
        let count = *self.concurrent.get(&ip).unwrap_or(&0);
        if count >= self.max_concurrent {
            return false;
        }
        let bucket = self
            .rate
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.burst, self.per_sec));
        if !bucket.try_take() {
            return false;
        }
        self.concurrent.insert(ip, count + 1);
        true
    }

    fn release(&mut self, ip: IpAddr) {
        if let Some(count) = self.concurrent.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.concurrent.remove(&ip);
            }
        }
    }
}

impl Default for IpLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Releases an IP's concurrency slot when the connection task ends.
pub struct IpGuard {
    limiter: Arc<Mutex<IpLimiter>>,
    ip: IpAddr,
}

impl IpGuard {
    pub fn new(limiter: Arc<Mutex<IpLimiter>>, ip: IpAddr) -> Self {
        Self { limiter, ip }
    }
}

impl Drop for IpGuard {
    fn drop(&mut self) {
        if let Ok(mut limiter) = self.limiter.lock() {
            limiter.release(self.ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_bucket_drains_then_refuses() {
        let mut bucket = TokenBucket::new(2.0, 0.0); // no refill
        assert!(bucket.try_take());
        assert!(bucket.try_take());
        assert!(!bucket.try_take());
    }

    #[test]
    fn ip_limiter_caps_concurrency_and_releases() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        // Cap 2 concurrent, generous rate so only concurrency bites.
        let mut limiter = IpLimiter::with_limits(2, 100.0, 100.0);
        assert!(limiter.admit(ip));
        assert!(limiter.admit(ip));
        assert!(!limiter.admit(ip), "third exceeds the concurrency cap");
        limiter.release(ip);
        assert!(limiter.admit(ip), "a freed slot is reusable");
    }

    #[test]
    fn ip_limiter_caps_new_connection_rate() {
        let ip: IpAddr = "10.0.0.2".parse().unwrap();
        // High concurrency cap, rate bucket of 3 with no refill.
        let mut limiter = IpLimiter::with_limits(100, 3.0, 0.0);
        assert!(limiter.admit(ip));
        assert!(limiter.admit(ip));
        assert!(limiter.admit(ip));
        assert!(!limiter.admit(ip), "fourth exceeds the new-connection rate");
    }
}
