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

/// The smallest ID [`TimeOrderedIds`] hands out at `ms`: stands for
/// "everything before this moment".
pub fn id_floor(ms: i64) -> u64 {
    (ms.max(0) as u64) << 20
}

impl TimeOrderedIds {
    pub fn new(last: u64) -> Self {
        Self { last }
    }

    pub fn next(&mut self, now_ms: i64) -> u64 {
        let candidate = id_floor(now_ms) | rand::rng().random_range(0..1 << 19);
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

/// What a service is, as written in its certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Account,
    Directory,
    MessageCell,
    TranslationCell,
    Engine,
    Media,
    Edge,
    /// Operators' tools (moving guilds, for instance).
    Admin,
    /// Another platform using a Translation Cell; the name is its tenant.
    Tenant,
}

impl Role {
    pub const ALL: [Role; 9] = [
        Role::Account,
        Role::Directory,
        Role::MessageCell,
        Role::TranslationCell,
        Role::Engine,
        Role::Media,
        Role::Edge,
        Role::Admin,
        Role::Tenant,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Account => "account",
            Role::Directory => "directory",
            Role::MessageCell => "message-cell",
            Role::TranslationCell => "translation-cell",
            Role::Engine => "engine",
            Role::Media => "media",
            Role::Edge => "edge",
            Role::Admin => "admin",
            Role::Tenant => "tenant",
        }
    }

    pub fn parse(role: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|known| known.as_str() == role)
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A service instance: its role and its name in the cluster
/// (`message-cell/message-cell-0`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    pub role: Role,
    pub name: String,
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.role, self.name)
    }
}

tokio::task_local! {
    static CALLER: Identity;
}

/// Runs `future` as a request from `caller`. The network layer does this
/// for every request it has authenticated.
pub async fn with_caller<F: Future>(caller: Identity, future: F) -> F::Output {
    CALLER.scope(caller, future).await
}

/// Who sent the request being served, proven by their certificate. `None`
/// for calls within one process.
pub fn caller() -> Option<Identity> {
    CALLER.try_with(Clone::clone).ok()
}

/// Remembers how far logs had grown at points in time, so that what is older
/// than a retention period can be trimmed without a timestamp on every
/// entry. One mark per log and trimming pass; marks are kept in memory, so
/// after a restart nothing is trimmed until a retention period has passed.
pub struct LogMarks<K> {
    marks: Mutex<HashMap<K, std::collections::VecDeque<(std::time::Instant, i64)>>>,
}

impl<K> Default for LogMarks<K> {
    fn default() -> Self {
        Self {
            marks: Mutex::default(),
        }
    }
}

impl<K: Eq + Hash + Clone> LogMarks<K> {
    /// Records that the log of `key` ends at `head` now. Returns where it
    /// ended `retention` ago (the newest mark at least that old): entries up
    /// to there may go.
    pub fn mark(&self, key: &K, head: i64, retention: Duration) -> Option<i64> {
        let now = std::time::Instant::now();
        let mut marks = self.marks.lock();
        let log = marks.entry(key.clone()).or_default();
        log.push_back((now, head));
        let mut old_enough = None;
        while let Some(&(at, seq)) = log.front() {
            if now.duration_since(at) < retention {
                break;
            }
            old_enough = Some(seq);
            // Keep the newest old-enough mark; later passes measure from it.
            if log.len() == 1 {
                break;
            }
            log.pop_front();
        }
        old_enough
    }

    /// Forgets a log that no longer exists.
    pub fn forget(&self, key: &K) {
        self.marks.lock().remove(key);
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

    #[test]
    fn marks_tell_where_logs_ended_a_retention_ago() {
        let marks = LogMarks::<u8>::default();
        let hour = Duration::from_secs(3600);
        assert_eq!(marks.mark(&1, 10, hour), None);
        assert_eq!(marks.mark(&1, 20, hour), None);
        // Without a retention, everything so far may go.
        assert_eq!(marks.mark(&1, 30, Duration::ZERO), Some(30));
        assert_eq!(marks.mark(&2, 5, Duration::ZERO), Some(5));
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
