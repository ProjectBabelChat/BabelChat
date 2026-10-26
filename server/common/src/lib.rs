//! Small helpers shared by BabelChat services.

use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use rand::Rng;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::watch;

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// A random identifier for guilds, users and sessions.
///
/// Identifiers are random on purpose: unlike Snowflake IDs they carry no
/// worker or cell number, so they reveal nothing about placement.
pub fn random_id() -> String {
    let bytes: [u8; 12] = rand::rng().random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Time-ordered identifiers for one single writer (a guild actor): the
/// millisecond timestamp in the high bits and random low bits, kept strictly
/// increasing.
#[derive(Debug, Default)]
pub struct TimeOrderedIds {
    last: u64,
}

impl TimeOrderedIds {
    pub fn new(last: u64) -> Self {
        Self { last }
    }

    pub fn next(&mut self, now_ms: i64) -> u64 {
        let candidate = ((now_ms.max(0) as u64) << 20) | rand::rng().random_range(0..1 << 19);
        self.last = candidate.max(self.last + 1);
        self.last
    }
}

/// Converts between the types two generated crates derived from the same
/// TYML file (a server crate and a client crate). Their JSON is identical by
/// construction.
pub fn convert<A: Serialize, B: DeserializeOwned>(value: &A) -> B {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .expect("types generated from the same TYML have the same JSON shape")
}

/// Wakes long polls when a log grows. One sequence number per key (a guild,
/// a space, a session).
pub struct LogSignals<K> {
    senders: Mutex<HashMap<K, watch::Sender<i64>>>,
}

impl<K> Default for LogSignals<K> {
    fn default() -> Self {
        Self {
            senders: Mutex::default(),
        }
    }
}

impl<K: Eq + Hash + Clone> LogSignals<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that the log of `key` now ends at `seq`.
    pub fn publish(&self, key: &K, seq: i64) {
        self.sender(key).send_if_modified(|current| {
            let grew = seq > *current;
            *current = (*current).max(seq);
            grew
        });
    }

    /// Waits until the log of `key` ends after `after`, or `timeout` passes.
    /// Returns whether it did.
    pub async fn wait_past(&self, key: &K, after: i64, timeout: Duration) -> bool {
        let mut receiver = self.sender(key).subscribe();
        let wait = receiver.wait_for(|seq| *seq > after);
        matches!(tokio::time::timeout(timeout, wait).await, Ok(Ok(_)))
    }

    fn sender(&self, key: &K) -> watch::Sender<i64> {
        self.senders
            .lock()
            .entry(key.clone())
            .or_insert_with(|| watch::channel(0).0)
            .clone()
    }
}

/// Clamps a client-supplied long poll duration.
pub fn poll_timeout(wait_ms: i64) -> Duration {
    Duration::from_millis(wait_ms.clamp(0, 30_000) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_increase_within_a_millisecond() {
        let mut ids = TimeOrderedIds::default();
        let a = ids.next(1_000);
        let b = ids.next(1_000);
        let c = ids.next(999);
        assert!(a < b && b < c);
    }

    #[tokio::test]
    async fn signals_wake_waiters() {
        let signals = std::sync::Arc::new(LogSignals::<u8>::new());
        signals.publish(&1, 3);
        assert!(signals.wait_past(&1, 2, Duration::ZERO).await);

        let waiter = {
            let signals = signals.clone();
            tokio::spawn(async move { signals.wait_past(&1, 3, Duration::from_secs(5)).await })
        };
        tokio::task::yield_now().await;
        signals.publish(&1, 4);
        assert!(waiter.await.unwrap());
        assert!(!signals.wait_past(&2, 0, Duration::from_millis(10)).await);
    }
}
