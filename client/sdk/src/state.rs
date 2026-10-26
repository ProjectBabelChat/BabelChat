//! What a session knows, and how gateway events change it.
//!
//! Everything here is plain data behind the session's lock; no I/O.

use std::collections::{BTreeMap, HashMap};

use babel_api_edge_client::types::{
    Channel, Event, EventKind, GuildView, Member, Message, MessageTranslation, UserSettings,
};

use crate::session::{GuildChange, MessageChange, Update};

/// Messages kept per channel; older ones are dropped and reloaded on demand.
const CHANNEL_LIMIT: usize = 2000;
/// Translations that arrived before their message (the Edge follows the
/// message and translation logs separately).
const ORPHAN_LIMIT: usize = 1000;

#[derive(Default)]
pub(crate) struct State {
    pub settings: Option<UserSettings>,
    pub guilds: Vec<GuildView>,
    pub channels: HashMap<String, ChannelLog>,
    orphans: HashMap<String, MessageTranslation>,
    /// Device results by job: the translated text, or why it failed.
    pub device: HashMap<crate::display::DeviceJob, Result<String, String>>,
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

    pub fn remove_message(&mut self, channel: &str, message: &str) -> bool {
        self.channels
            .get_mut(channel)
            .and_then(|log| log.messages.remove(&MessageKey::new(message)))
            .is_some()
    }

    /// Applies one gateway event; returns what the app should hear about.
    pub fn apply(&mut self, event: Event) -> Vec<Update> {
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
                self.put_message(message)
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
                if !self.remove_message(&deleted.channel_id, &deleted.message_id) {
                    return Vec::new();
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
