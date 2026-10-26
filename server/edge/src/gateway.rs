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
/// Sessions not polled for this long are dropped.
pub const SESSION_IDLE: Duration = Duration::from_secs(10 * 60);

pub struct Session {
    pub user_id: String,
    pub settings: UserSettings,
    pub guilds: HashSet<String>,
    events: VecDeque<Event>,
    next_seq: i64,
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
        member: None,
        channel: None,
        message: None,
        deleted: None,
        translation: None,
        policy: None,
        language: None,
    }
}

impl Sessions {
    pub fn create(&self, id: &str, user_id: &str, settings: UserSettings) {
        self.sessions.lock().insert(
            id.to_string(),
            Session {
                user_id: user_id.to_string(),
                settings,
                guilds: HashSet::new(),
                events: VecDeque::new(),
                next_seq: 1,
                last_seen: Instant::now(),
            },
        );
    }

    /// Adds a guild to every session of `user_id` (or one session), so they
    /// start receiving its events.
    pub fn add_guild(&self, user_id: &str, only: Option<&str>, guild: &str) {
        let mut sessions = self.sessions.lock();
        let mut by_guild = self.by_guild.lock();
        for (id, session) in sessions.iter_mut() {
            if session.user_id == user_id && only.is_none_or(|only| only == id) {
                session.guilds.insert(guild.to_string());
                by_guild
                    .entry(guild.to_string())
                    .or_default()
                    .insert(id.clone());
            }
        }
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

    /// Events after `after`, waiting up to `timeout` for the first one.
    /// `None` if the session does not exist or belongs to someone else.
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

    /// Drops sessions that stopped polling.
    pub fn sweep(&self) {
        let mut sessions = self.sessions.lock();
        let idle = sessions
            .iter()
            .filter(|(_, session)| session.last_seen.elapsed() > SESSION_IDLE)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut by_guild = self.by_guild.lock();
        for id in idle {
            if let Some(session) = sessions.remove(&id) {
                for guild in session.guilds {
                    if let Some(ids) = by_guild.get_mut(&guild) {
                        ids.remove(&id);
                    }
                }
            }
        }
    }
}

fn push(session: &mut Session, mut event: Event) -> i64 {
    event.seq = session.next_seq;
    session.next_seq += 1;
    session.events.push_back(event);
    while session.events.len() > QUEUE_LIMIT {
        session.events.pop_front();
    }
    session.next_seq - 1
}
