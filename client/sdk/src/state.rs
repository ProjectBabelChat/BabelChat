//! What a session knows, and how gateway events change it.
//!
//! Everything here is plain data behind the session's lock; no I/O.

use std::collections::{BTreeMap, HashMap};

use babel_api_edge_client::types::{
    Channel, Event, EventKind, GuildView, Member, Message, MessageTranslation, NotifyLevel,
    ReadState, UserSettings,
};

use crate::session::{GuildChange, MessageChange, NotifyReason, Unread, Update};

/// Messages kept per channel; older ones are dropped and reloaded on demand.
const CHANNEL_LIMIT: usize = 2000;
/// Translations that arrived before their message (the Edge follows the
/// message and translation logs separately).
const ORPHAN_LIMIT: usize = 1000;

#[derive(Default)]
pub(crate) struct State {
    pub user_id: String,
    pub settings: Option<UserSettings>,
    pub guilds: Vec<GuildView>,
    pub channels: HashMap<String, ChannelLog>,
    orphans: HashMap<String, MessageTranslation>,
    /// Device results by job: the translated text, or why it failed.
    pub device: HashMap<crate::display::DeviceJob, Result<String, String>>,
    /// When device jobs failed in ways that may pass (not "no model").
    pub device_failed: HashMap<crate::display::DeviceJob, std::time::Instant>,
}

#[derive(Default)]
pub(crate) struct ChannelLog {
    pub guild_id: String,
    /// By time-ordered ID.
    pub messages: BTreeMap<MessageKey, Entry>,
    /// History has been loaded, so the newest page is worth reloading
    /// after a reconnect.
    pub loaded: bool,
    /// Nothing older exists on the server.
    pub complete: bool,
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub message: Message,
    pub translation: Option<MessageTranslation>,
}

/// Message IDs are time-ordered integers; sort numerically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct MessageKey(u64, String);

impl MessageKey {
    pub fn new(id: &str) -> Self {
        Self(id.parse().unwrap_or_default(), id.to_string())
    }
}

/// IDs compare as numbers; read markers may name no message.
pub(crate) fn id_number(id: &str) -> u64 {
    id.parse().unwrap_or_default()
}

impl State {
    pub fn guild(&self, id: &str) -> Option<&GuildView> {
        self.guilds.iter().find(|view| view.guild.id == id)
    }

    fn guild_mut(&mut self, id: &str) -> Option<&mut GuildView> {
        self.guilds.iter_mut().find(|view| view.guild.id == id)
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.guilds
            .iter()
            .flat_map(|view| &view.channels)
            .find(|channel| channel.id == id)
    }

    pub fn entry(&self, channel: &str, message: &str) -> Option<&Entry> {
        self.channels
            .get(channel)?
            .messages
            .get(&MessageKey::new(message))
    }

    fn log(&mut self, guild: &str, channel: &str) -> &mut ChannelLog {
        let log = self.channels.entry(channel.to_string()).or_default();
        log.guild_id = guild.to_string();
        log
    }

    /// Adds or replaces a guild. True if it is new.
    pub fn put_guild(&mut self, view: GuildView) -> bool {
        match self.guild_mut(&view.guild.id) {
            Some(known) => {
                *known = view;
                false
            }
            None => {
                self.guilds.push(view);
                true
            }
        }
    }

    /// Forgets a guild the user is no longer in. True if it was known.
    pub fn remove_guild(&mut self, guild: &str) -> bool {
        let known = self.guild(guild).is_some();
        self.guilds.retain(|view| view.guild.id != guild);
        self.channels.retain(|_, log| log.guild_id != guild);
        known
    }

    // ---- Reading --------------------------------------------------------

    pub fn read_state(&self, channel: &str) -> Option<&ReadState> {
        self.guilds
            .iter()
            .flat_map(|view| &view.read_states)
            .find(|state| state.channel_id == channel)
    }

    /// The read state of a known channel, created for channels that
    /// appeared after the guild was loaded (all of it unread).
    fn read_state_mut(&mut self, guild: &str, channel: &str) -> Option<&mut ReadState> {
        let view = self.guild_mut(guild)?;
        let at = match view
            .read_states
            .iter()
            .position(|state| state.channel_id == channel)
        {
            Some(at) => at,
            None => {
                view.read_states.push(ReadState {
                    channel_id: channel.to_string(),
                    last_read_id: "0".into(),
                    mention_count: 0,
                });
                view.read_states.len() - 1
            }
        };
        view.read_states.get_mut(at)
    }

    pub fn unread(&self, channel: &str) -> Unread {
        let newest = self
            .channel(channel)
            .and_then(|channel| channel.last_message_id.as_deref())
            .map(id_number)
            .unwrap_or(0);
        match self.read_state(channel) {
            Some(state) => Unread {
                unread: newest > id_number(&state.last_read_id),
                mentions: state.mention_count,
            },
            None => Unread {
                unread: newest > 0,
                mentions: 0,
            },
        }
    }

    /// Takes a read state from the server unless it is behind what is
    /// known already. True if it changed anything.
    pub fn put_read_state(&mut self, guild: &str, state: ReadState) -> bool {
        let Some(known) = self.read_state_mut(guild, &state.channel_id) else {
            return false;
        };
        if id_number(&state.last_read_id) < id_number(&known.last_read_id) || *known == state {
            return false;
        }
        *known = state;
        true
    }

    /// Counts a mention of this user in a message they have not read.
    fn count_mention(&mut self, guild: &str, channel: &str, id: &str, delta: i64) {
        if let Some(state) = self.read_state_mut(guild, channel)
            && id_number(id) > id_number(&state.last_read_id)
        {
            state.mention_count = (state.mention_count + delta).max(0);
        }
    }

    /// Keeps read markers in step with a new message, as the server does:
    /// the channel's newest message moves, writing reads the channel, and
    /// mentions of this user count until read.
    fn track_new(&mut self, message: &Message) {
        let (guild, channel) = (message.guild_id.as_str(), message.channel_id.as_str());
        if let Some(known) = self
            .guild_mut(guild)
            .and_then(|view| view.channels.iter_mut().find(|known| known.id == channel))
        {
            let newer = known
                .last_message_id
                .as_deref()
                .is_none_or(|last| id_number(last) < id_number(&message.id));
            if newer {
                known.last_message_id = Some(message.id.clone());
            }
        }
        if message.author_id == self.user_id {
            if let Some(state) = self.read_state_mut(guild, channel)
                && id_number(&message.id) > id_number(&state.last_read_id)
            {
                state.last_read_id = message.id.clone();
                state.mention_count = 0;
            }
        } else if message.mentions.contains(&self.user_id) {
            self.count_mention(guild, channel, &message.id, 1);
        }
    }

    // ---- Notifications --------------------------------------------------

    /// The level that applies to a channel: its own entry, else the nearest
    /// parent's (a thread's channel, a channel's category), else the
    /// guild's.
    pub fn notify_level(&self, guild: &str, channel: &str) -> NotifyLevel {
        let Some(view) = self.guild(guild) else {
            return NotifyLevel::All;
        };
        let settings = &view.notifications;
        let mut next = Some(channel.to_string());
        while let Some(id) = next {
            if let Some(entry) = settings.channels.iter().find(|e| e.channel_id == id) {
                return entry.level;
            }
            next = view
                .channels
                .iter()
                .find(|known| known.id == id)
                .and_then(|known| known.parent_id.clone());
        }
        settings.level
    }

    /// Whether a message that just arrived should notify this user.
    pub fn notify_reason(&self, message: &Message, focused: Option<&str>) -> Option<NotifyReason> {
        if message.author_id == self.user_id || focused == Some(message.channel_id.as_str()) {
            return None;
        }
        let mentioned = message.mentions.contains(&self.user_id);
        let reason = if !mentioned {
            NotifyReason::Message
        } else if message
            .reply_to
            .as_ref()
            .and_then(|id| self.entry(&message.channel_id, id))
            .is_some_and(|replied| replied.message.author_id == self.user_id)
        {
            NotifyReason::Reply
        } else {
            NotifyReason::Mention
        };
        match self.notify_level(&message.guild_id, &message.channel_id) {
            NotifyLevel::All => Some(reason),
            NotifyLevel::Mentions if mentioned => Some(reason),
            _ => None,
        }
    }

    // ---- Messages -------------------------------------------------------

    /// Adds a message or a newer revision of it. Returns what changed.
    pub fn put_message(&mut self, message: Message) -> Option<MessageChange> {
        let orphan = self
            .orphans
            .remove(&message.id)
            .filter(|t| t.rev == message.rev);
        let log = self.log(&message.guild_id, &message.channel_id);
        let key = MessageKey::new(&message.id);
        match log.messages.get_mut(&key) {
            Some(entry) if entry.message.rev >= message.rev => None,
            Some(entry) => {
                entry.message = message;
                Some(MessageChange::Edited)
            }
            None => {
                log.messages.insert(
                    key,
                    Entry {
                        message,
                        translation: orphan,
                    },
                );
                while log.messages.len() > CHANNEL_LIMIT {
                    log.messages.pop_first();
                    log.complete = false;
                }
                Some(MessageChange::Created)
            }
        }
    }

    /// A message as it happens (from the gateway or this user's own
    /// writes, not history): read markers move with it.
    pub fn put_live(&mut self, message: Message) -> Option<MessageChange> {
        let mentioned_before = self
            .entry(&message.channel_id, &message.id)
            .map(|entry| entry.message.mentions.contains(&self.user_id));
        let change = self.put_message(message.clone());
        match (change, mentioned_before) {
            (Some(MessageChange::Created), _) => self.track_new(&message),
            (Some(MessageChange::Edited), Some(before)) => {
                let now = message.mentions.contains(&self.user_id);
                if before != now {
                    let delta = if now { 1 } else { -1 };
                    self.count_mention(&message.guild_id, &message.channel_id, &message.id, delta);
                }
            }
            _ => {}
        }
        change
    }

    /// Attaches a translation unless a newer one is known. True if it did.
    pub fn put_translation(&mut self, translation: MessageTranslation) -> bool {
        let key = MessageKey::new(&translation.message_id);
        let Some(entry) = self
            .channels
            .get_mut(&translation.channel_id)
            .and_then(|log| log.messages.get_mut(&key))
        else {
            if self.orphans.len() >= ORPHAN_LIMIT {
                self.orphans.clear();
            }
            self.orphans
                .insert(translation.message_id.clone(), translation);
            return false;
        };
        let older = entry.translation.as_ref().is_some_and(|known| {
            known.rev > translation.rev
                || (known.rev == translation.rev
                    && known.status == "done"
                    && translation.status != "done")
        });
        if older {
            return false;
        }
        entry.translation = Some(translation);
        true
    }

    pub fn remove_message(&mut self, channel: &str, message: &str) -> Option<Message> {
        self.channels
            .get_mut(channel)
            .and_then(|log| log.messages.remove(&MessageKey::new(message)))
            .map(|entry| entry.message)
    }

    // ---- Events ---------------------------------------------------------

    /// Applies one gateway event; returns what the app should hear about.
    pub fn apply(&mut self, event: Event) -> Vec<Update> {
        let guild_id = event.guild_id.clone();
        // Message events may move the channel's unread marker.
        let channel = event
            .message
            .as_ref()
            .map(|message| message.channel_id.clone())
            .or_else(|| {
                event
                    .deleted
                    .as_ref()
                    .map(|deleted| deleted.channel_id.clone())
            })
            .or_else(|| {
                event
                    .read_state
                    .as_ref()
                    .map(|state| state.channel_id.clone())
            });
        let before = channel.as_deref().map(|channel| self.unread(channel));

        let mut updates = self.apply_event(event);

        if let (Some(channel), Some(before)) = (channel, before)
            && self.unread(&channel) != before
        {
            updates.push(Update::Unread {
                guild_id,
                channel_id: channel,
            });
        }
        updates
    }

    fn apply_event(&mut self, event: Event) -> Vec<Update> {
        let guild_id = event.guild_id.clone();
        let guild_update = |change| {
            vec![Update::Guild {
                guild_id: guild_id.clone(),
                change,
            }]
        };
        match event.kind {
            EventKind::GuildJoined => {
                let Some(view) = event.guild else {
                    return Vec::new();
                };
                let added = self.put_guild(view);
                guild_update(if added {
                    GuildChange::Joined
                } else {
                    GuildChange::Refreshed
                })
            }
            EventKind::MemberJoined => {
                let (Some(member), Some(view)) = (event.member, self.guild_mut(&guild_id)) else {
                    return Vec::new();
                };
                let user_id = member.user_id.clone();
                if !add_member(&mut view.members, member) {
                    return Vec::new();
                }
                guild_update(GuildChange::MemberJoined { user_id })
            }
            EventKind::MemberLeft | EventKind::MemberRemoved => {
                let Some(member) = event.member else {
                    return Vec::new();
                };
                let removed = event.kind == EventKind::MemberRemoved;
                if member.user_id == self.user_id {
                    if !self.remove_guild(&guild_id) {
                        return Vec::new();
                    }
                    return guild_update(if removed {
                        GuildChange::Removed
                    } else {
                        GuildChange::Left
                    });
                }
                let Some(view) = self.guild_mut(&guild_id) else {
                    return Vec::new();
                };
                let count = view.members.len();
                view.members.retain(|known| known.user_id != member.user_id);
                if view.members.len() == count {
                    return Vec::new();
                }
                let user_id = member.user_id;
                guild_update(if removed {
                    GuildChange::MemberRemoved { user_id }
                } else {
                    GuildChange::MemberLeft { user_id }
                })
            }
            EventKind::ChannelCreated => {
                let (Some(channel), Some(view)) = (event.channel, self.guild_mut(&guild_id)) else {
                    return Vec::new();
                };
                if view.channels.iter().any(|known| known.id == channel.id) {
                    return Vec::new();
                }
                let channel_id = channel.id.clone();
                view.channels.push(channel);
                guild_update(GuildChange::ChannelCreated { channel_id })
            }
            EventKind::MessageCreated | EventKind::MessageEdited => {
                let Some(message) = event.message else {
                    return Vec::new();
                };
                let (channel_id, message_id) = (message.channel_id.clone(), message.id.clone());
                self.put_live(message)
                    .map(|change| Update::Message {
                        guild_id: guild_id.clone(),
                        channel_id,
                        message_id,
                        change,
                    })
                    .into_iter()
                    .collect()
            }
            EventKind::MessageDeleted => {
                let Some(deleted) = event.deleted else {
                    return Vec::new();
                };
                let Some(message) = self.remove_message(&deleted.channel_id, &deleted.message_id)
                else {
                    return Vec::new();
                };
                if message.mentions.contains(&self.user_id) {
                    self.count_mention(&guild_id, &message.channel_id, &message.id, -1);
                }
                vec![Update::Message {
                    guild_id,
                    channel_id: deleted.channel_id,
                    message_id: deleted.message_id,
                    change: MessageChange::Deleted,
                }]
            }
            EventKind::MessageTranslated => {
                let Some(translation) = event.translation else {
                    return Vec::new();
                };
                let (channel_id, message_id) = (
                    translation.channel_id.clone(),
                    translation.message_id.clone(),
                );
                if !self.put_translation(translation) {
                    return Vec::new();
                }
                vec![Update::Message {
                    guild_id,
                    channel_id,
                    message_id,
                    change: MessageChange::Translated,
                }]
            }
            EventKind::PolicyUpdated => {
                let (Some(policy), Some(language), Some(view)) =
                    (event.policy, event.language, self.guild_mut(&guild_id))
                else {
                    return Vec::new();
                };
                view.policy = policy;
                view.language = language;
                guild_update(GuildChange::Languages)
            }
            // `apply` reports the unread change.
            EventKind::ReadStateUpdated => {
                if let Some(state) = event.read_state {
                    self.put_read_state(&guild_id, state);
                }
                Vec::new()
            }
            EventKind::NotificationsUpdated => {
                let (Some(settings), Some(view)) = (event.notifications, self.guild_mut(&guild_id))
                else {
                    return Vec::new();
                };
                if view.notifications == settings {
                    return Vec::new();
                }
                view.notifications = settings;
                guild_update(GuildChange::Notifications)
            }
        }
    }

    /// Merges a page of history (newest first, as the server sends it).
    /// With `authoritative`, messages inside the page's range that the page
    /// lacks were deleted while nobody was listening.
    pub fn merge_page(
        &mut self,
        guild: &str,
        channel: &str,
        page: Vec<(Message, Option<MessageTranslation>)>,
        limit: usize,
        authoritative: bool,
    ) -> usize {
        let full = page.len() >= limit;
        let log = self.log(guild, channel);
        log.loaded = true;
        if !full {
            log.complete = true;
        }
        if let (true, Some((newest, _)), Some((oldest, _))) =
            (authoritative, page.first(), page.last())
        {
            let range = MessageKey::new(&oldest.id)..=MessageKey::new(&newest.id);
            let keep = page
                .iter()
                .map(|(message, _)| MessageKey::new(&message.id))
                .collect::<std::collections::HashSet<_>>();
            let gone = log
                .messages
                .range(range)
                .map(|(key, _)| key)
                .filter(|key| !keep.contains(key))
                .cloned()
                .collect::<Vec<_>>();
            for key in gone {
                log.messages.remove(&key);
            }
        }
        let count = page.len();
        for (message, translation) in page {
            self.put_message(message);
            if let Some(translation) = translation {
                self.put_translation(translation);
            }
        }
        count
    }
}

fn add_member(members: &mut Vec<Member>, member: Member) -> bool {
    if members.iter().any(|known| known.user_id == member.user_id) {
        return false;
    }
    members.push(member);
    true
}
