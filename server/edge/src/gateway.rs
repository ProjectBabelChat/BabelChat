//! Sessions and fan-out.
//!
//! An Edge follows each guild its users are in exactly once: one long poll on
//! the guild's Message Cell log and one on its Translation Cell log, however
//! many sessions read the guild. Each event is then copied into the queue of
//! every session in that guild, filtered for that session's language.
//!
//! Phase 0 delivers queues by long polling; a WebSocket would drain the same
//! queues.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use babel_api_edge::types::{Event, EventKind, UserSettings};
use babel_common::LogSignals;
use parking_lot::Mutex;

/// Events kept per session for a client to catch up after a short gap.
const QUEUE_LIMIT: usize = 1000;
/// Subscribed, but the snapshot is not taken yet.
const PENDING: i64 = i64::MAX;
/// Sessions not polled for this long are dropped.
pub const SESSION_IDLE: Duration = Duration::from_secs(10 * 60);

pub struct Session {
    pub user_id: String,
    pub settings: UserSettings,
    /// Guild -> sequence number of the snapshot the session started from
    /// (`PENDING` until it is taken).
    pub guilds: HashMap<String, i64>,
    events: VecDeque<Event>,
    next_seq: i64,
    /// Newest event pushed out of the queue unread.
    dropped: i64,
    last_seen: Instant,
}

#[derive(Default)]
pub struct Sessions {
    sessions: Mutex<HashMap<String, Session>>,
    /// Guild -> sessions reading it.
    by_guild: Mutex<HashMap<String, HashSet<String>>>,
    signals: LogSignals<String>,
}

/// An event before it gets its per-session sequence number.
pub fn event(kind: EventKind, guild_id: &str) -> Event {
    Event {
        seq: 0,
        kind,
        guild_id: guild_id.to_string(),
        guild: None,
        guild_info: None,
        member: None,
        channel: None,
        message: None,
        deleted: None,
        reaction: None,
        emoji: None,
        typing: None,
        translation: None,
        policy: None,
        language: None,
        read_state: None,
        notifications: None,
    }
}

impl Sessions {
    pub fn create(&self, id: &str, user_id: &str, settings: UserSettings) {
        let mut sessions = self.sessions.lock();
        sessions.insert(
            id.to_string(),
            Session {
                user_id: user_id.to_string(),
                settings,
                guilds: HashMap::new(),
                events: VecDeque::new(),
                next_seq: 1,
                dropped: 0,
                last_seen: Instant::now(),
            },
        );
        metrics::gauge!("babel_edge_sessions").set(sessions.len() as f64);
    }

    /// Adds a guild to every session of `user_id` (or one session), so they
    /// start receiving its events. Follow with [`Self::confirm_guild`] once
    /// the snapshot is taken, or [`Self::remove_guild`] if it failed.
    pub fn add_guild(&self, user_id: &str, only: Option<&str>, guild: &str) {
        let mut sessions = self.sessions.lock();
        let mut by_guild = self.by_guild.lock();
        for (id, session) in sessions.iter_mut() {
            if session.user_id == user_id && only.is_none_or(|only| only == id) {
                session.guilds.entry(guild.to_string()).or_insert(PENDING);
                by_guild
                    .entry(guild.to_string())
                    .or_default()
                    .insert(id.clone());
            }
        }
    }

    /// Records the snapshot pending subscriptions started from.
    pub fn confirm_guild(&self, user_id: &str, guild: &str, seq: i64) {
        for session in self.sessions.lock().values_mut() {
            if session.user_id == user_id
                && let Some(from) = session.guilds.get_mut(guild)
                && *from == PENDING
            {
                *from = seq;
            }
        }
    }

    /// Undoes [`Self::add_guild`] when the snapshot could not be taken:
    /// the user may not see the guild (any more).
    pub fn cancel_guild(&self, user_id: &str, guild: &str) {
        self.remove_guild_where(user_id, guild, |from| from == PENDING);
    }

    /// Stops `user_id`'s sessions receiving `guild` after the guild event
    /// `seq` removed them. Subscriptions from a later snapshot (they joined
    /// again since) stay.
    pub fn remove_guild(&self, user_id: &str, guild: &str, seq: i64) {
        self.remove_guild_where(user_id, guild, |from| from < seq);
    }

    fn remove_guild_where(&self, user_id: &str, guild: &str, stale: impl Fn(i64) -> bool) {
        let mut sessions = self.sessions.lock();
        let mut by_guild = self.by_guild.lock();
        for (id, session) in sessions.iter_mut() {
            if session.user_id == user_id
                && session.guilds.get(guild).is_some_and(|from| stale(*from))
            {
                session.guilds.remove(guild);
                if let Some(ids) = by_guild.get_mut(guild) {
                    ids.remove(id);
                }
            }
        }
    }

    /// Takes `guild` from every session: it was deleted.
    pub fn forget_guild(&self, guild: &str) {
        let ids = self.by_guild.lock().remove(guild).unwrap_or_default();
        let mut sessions = self.sessions.lock();
        for id in ids {
            if let Some(session) = sessions.get_mut(&id) {
                session.guilds.remove(guild);
            }
        }
    }

    /// Whether `id` is a live session of `user_id`.
    pub fn owns(&self, id: &str, user_id: &str) -> bool {
        self.sessions
            .lock()
            .get(id)
            .is_some_and(|session| session.user_id == user_id)
    }

    pub fn update_settings(&self, user_id: &str, settings: &UserSettings) {
        for session in self.sessions.lock().values_mut() {
            if session.user_id == user_id {
                session.settings = settings.clone();
            }
        }
    }

    /// Pushes an event to every session reading `guild`. `build` returns the
    /// event as that session should see it, or `None` to skip the session.
    pub fn broadcast(&self, guild: &str, build: impl Fn(&Session) -> Option<Event>) {
        let ids = self.by_guild.lock().get(guild).cloned().unwrap_or_default();
        let mut woken = Vec::new();
        {
            let mut sessions = self.sessions.lock();
            for id in ids {
                let Some(session) = sessions.get_mut(&id) else {
                    continue;
                };
                if let Some(event) = build(session) {
                    woken.push((id, push(session, event)));
                }
            }
        }
        for (id, seq) in woken {
            self.signals.publish(&id, seq);
        }
    }

    /// Pushes an event to the sessions of one user.
    pub fn send_to_user(&self, user_id: &str, event: Event) {
        let mut woken = Vec::new();
        {
            let mut sessions = self.sessions.lock();
            for (id, session) in sessions.iter_mut() {
                if session.user_id == user_id {
                    woken.push((id.clone(), push(session, event.clone())));
                }
            }
        }
        for (id, seq) in woken {
            self.signals.publish(&id, seq);
        }
    }

    /// Sessions of `user_id` that do not read `guild`.
    pub fn without_guild(&self, user_id: &str, guild: &str) -> Vec<String> {
        self.sessions
            .lock()
            .iter()
            .filter(|(_, session)| {
                session.user_id == user_id && !session.guilds.contains_key(guild)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Pushes an event to the sessions `ids`.
    pub fn send_to_sessions(&self, ids: &[String], event: Event) {
        let mut woken = Vec::new();
        {
            let mut sessions = self.sessions.lock();
            for id in ids {
                if let Some(session) = sessions.get_mut(id) {
                    woken.push((id.clone(), push(session, event.clone())));
                }
            }
        }
        for (id, seq) in woken {
            self.signals.publish(&id, seq);
        }
    }

    /// Events after `after`, waiting up to `timeout` for the first one.
    /// `None` if the session does not exist, belongs to someone else, or
    /// has dropped events the client has not read; the client must then
    /// connect again and reload.
    pub async fn poll(
        &self,
        id: &str,
        user_id: &str,
        after: i64,
        timeout: Duration,
    ) -> Option<(Vec<Event>, i64)> {
        let read = || -> Option<(Vec<Event>, i64)> {
            let mut sessions = self.sessions.lock();
            let session = sessions.get_mut(id).filter(|s| s.user_id == user_id)?;
            if after < session.dropped {
                drop(sessions);
                self.remove(&[id.to_string()]);
                return None;
            }
            session.last_seen = Instant::now();
            let events = session
                .events
                .iter()
                .filter(|event| event.seq > after)
                .cloned()
                .collect::<Vec<_>>();
            let next = events.last().map(|event| event.seq).unwrap_or(after);
            Some((events, next))
        };

        let (events, next) = read()?;
        if !events.is_empty() {
            return Some((events, next));
        }
        self.signals
            .wait_past(&id.to_string(), after, timeout)
            .await;
        read()
    }

    /// Drops every session reading `guild`: they missed some of its events
    /// and must connect again and reload. Returns how many there were.
    pub fn drop_guild(&self, guild: &str) -> usize {
        let ids = self
            .by_guild
            .lock()
            .get(guild)
            .map(|ids| ids.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        self.remove(&ids);
        metrics::counter!("babel_edge_sessions_dropped_total").increment(ids.len() as u64);
        ids.len()
    }

    /// Drops every session, for the same reason.
    pub fn drop_all(&self) -> usize {
        let ids = self.sessions.lock().keys().cloned().collect::<Vec<_>>();
        self.remove(&ids);
        metrics::counter!("babel_edge_sessions_dropped_total").increment(ids.len() as u64);
        ids.len()
    }

    /// Drops every session of `user_id`: their tokens were revoked.
    pub fn drop_user(&self, user_id: &str) -> usize {
        let ids = self
            .sessions
            .lock()
            .iter()
            .filter(|(_, session)| session.user_id == user_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        self.remove(&ids);
        ids.len()
    }

    /// Drops sessions that stopped polling.
    pub fn sweep(&self) {
        let idle = self
            .sessions
            .lock()
            .iter()
            .filter(|(_, session)| session.last_seen.elapsed() > SESSION_IDLE)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        self.remove(&idle);
    }

    pub fn remove(&self, ids: &[String]) {
        let mut sessions = self.sessions.lock();
        let mut by_guild = self.by_guild.lock();
        for id in ids {
            if let Some(session) = sessions.remove(id) {
                for guild in session.guilds.into_keys() {
                    if let Some(ids) = by_guild.get_mut(&guild) {
                        ids.remove(id);
                    }
                }
                // Wake its poll or socket, which finds it gone.
                self.signals.publish(id, i64::MAX);
            }
        }
        metrics::gauge!("babel_edge_sessions").set(sessions.len() as f64);
    }
}

fn push(session: &mut Session, mut event: Event) -> i64 {
    event.seq = session.next_seq;
    session.next_seq += 1;
    session.events.push_back(event);
    while session.events.len() > QUEUE_LIMIT {
        if let Some(dropped) = session.events.pop_front() {
            session.dropped = dropped.seq;
        }
    }
    session.next_seq - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> UserSettings {
        UserSettings {
            locale: "en-US".into(),
            translate_outside_set: false,
            show_original: false,
        }
    }

    #[tokio::test]
    async fn sessions_that_fall_behind_must_reconnect() {
        let sessions = Sessions::default();
        sessions.create("s", "u", settings());
        for _ in 0..QUEUE_LIMIT + 1 {
            sessions.send_to_user("u", event(EventKind::GuildJoined, "g"));
        }

        // Read past the dropped event: fine.
        let (events, next) = sessions.poll("s", "u", 1, Duration::ZERO).await.unwrap();
        assert_eq!(events.len(), QUEUE_LIMIT);
        assert_eq!(next, QUEUE_LIMIT as i64 + 1);

        // Behind it: the session is gone, so the client reloads.
        assert!(sessions.poll("s", "u", 0, Duration::ZERO).await.is_none());
        assert!(
            sessions
                .poll("s", "u", next, Duration::ZERO)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn removals_spare_sessions_that_joined_again() {
        let sessions = Sessions::default();
        sessions.create("s", "u", settings());
        let message = || Some(event(EventKind::MessageCreated, "g"));
        let sessions = &sessions;
        let received = |after| async move {
            sessions
                .poll("s", "u", after, Duration::ZERO)
                .await
                .unwrap()
                .0
                .len()
        };

        sessions.add_guild("u", None, "g");
        sessions.confirm_guild("u", "g", 10);
        // Removed at 7, then joined again: the snapshot at 10 is newer.
        sessions.remove_guild("u", "g", 7);
        sessions.broadcast("g", |_| message());
        assert_eq!(received(0).await, 1);
        // Removed after the snapshot: no more events.
        sessions.remove_guild("u", "g", 11);
        sessions.broadcast("g", |_| message());
        assert_eq!(received(1).await, 0);

        // A snapshot that failed undoes the subscription.
        sessions.add_guild("u", None, "h");
        sessions.cancel_guild("u", "h");
        sessions.broadcast("h", |_| Some(event(EventKind::MessageCreated, "h")));
        assert_eq!(received(1).await, 0);
    }
}
