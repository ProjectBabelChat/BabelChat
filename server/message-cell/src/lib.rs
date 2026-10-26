//! The Message Cell: guilds, channels, members and original messages.
//!
//! One process holds many guilds. Each guild has a single writer (here, a
//! per-guild lock) and an event log. Every change is one atomic batch:
//!
//! ```text
//! state change + log entry (+ outbox entry)
//!        │            │              └─▶ outbox worker ─▶ Translation Cell, Directory
//!        │            └─▶ poll_events ─▶ Edges ─▶ clients
//!        └─▶ reads (get_guild, get_messages)
//! ```
//!
//! See docs/architecture/message-cell.md.

mod outbox;
mod records;

use std::{
    collections::HashMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

use async_trait::async_trait;
use babel_api_directory::types::Directory;
use babel_api_message_cell::types::{
    Channel, ChannelKind, DeletedMessage, Guild, GuildEvent, GuildEventBatch, GuildEventKind,
    GuildSnapshot, Member, Message, MessageCellError, MessageEdit, NewChannel, NewGuild,
    NewMessage, Permission, Role,
};
use babel_api_translation_cell::types::TranslationCell;
use babel_common::{LogSignals, TimeOrderedIds, now_ms, poll_timeout, random_id};
use babel_store::{Batch, KvStore, Scan, StoreError};
use parking_lot::Mutex;
pub use records::TABLES;
use records::{
    CHANNELS, GUILDS, GuildRecord, LOG, MEMBERS, MESSAGES, OUTBOX, OutboxEntry, channel_key,
    guild_key, log_key, member_key, message_key, message_prefix, outbox_key,
};
use tokio::sync::{Notify, OwnedMutexGuard};

const MAX_NAME_CHARS: usize = 100;
const MAX_TOPIC_CHARS: usize = 1000;
const MAX_TEXT_CHARS: usize = 4000;
const MAX_PAGE: i64 = 100;
const POLL_LIMIT: usize = 100;

type CellResult<T> = Result<T, MessageCellError>;

fn storage(error: StoreError) -> MessageCellError {
    tracing::error!(%error, "message cell storage failed");
    MessageCellError::Unavailable
}

fn event(kind: GuildEventKind) -> GuildEvent {
    GuildEvent {
        seq: 0,
        kind,
        guild: None,
        member: None,
        channel: None,
        message: None,
        deleted: None,
    }
}

fn check_text(text: &str, max: usize) -> CellResult<()> {
    let len = text.chars().count();
    if text.trim().is_empty() || len > max {
        return Err(MessageCellError::InvalidRequest);
    }
    Ok(())
}

#[derive(Clone)]
pub struct MessageCell {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) store: Arc<dyn KvStore>,
    /// Translation Cells by name; each guild records which one it uses.
    pub(crate) translation_cells: HashMap<String, Arc<dyn TranslationCell>>,
    /// Set once the Directory exists (it also needs the Message Cells).
    pub(crate) directory: OnceLock<Arc<dyn Directory>>,
    pub(crate) outbox_signal: Notify,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    signals: LogSignals<String>,
    next_outbox: AtomicI64,
}

impl MessageCell {
    /// Opens the cell and starts its outbox worker. Must be called within a
    /// Tokio runtime.
    pub fn open(
        store: Arc<dyn KvStore>,
        translation_cells: HashMap<String, Arc<dyn TranslationCell>>,
    ) -> Result<Self, StoreError> {
        let last = store.scan(Scan::prefix(OUTBOX, &[]).reverse().limit(1))?;
        let next_outbox = last
            .first()
            .and_then(|(key, _)| babel_store::KeyReader::new(key).u64())
            .map(|seq| seq as i64 + 1)
            .unwrap_or(1);

        let inner = Arc::new(Inner {
            store,
            translation_cells,
            directory: OnceLock::new(),
            outbox_signal: Notify::new(),
            locks: Mutex::default(),
            signals: LogSignals::new(),
            next_outbox: AtomicI64::new(next_outbox),
        });
        tokio::spawn(outbox::run(inner.clone()));
        Ok(Self { inner })
    }

    /// Gives the outbox somewhere to send membership changes.
    pub fn connect_directory(&self, directory: Arc<dyn Directory>) {
        let _ = self.inner.directory.set(directory);
        self.inner.outbox_signal.notify_one();
    }
}

impl Inner {
    async fn lock(&self, guild: &str) -> OwnedMutexGuard<()> {
        let lock = self
            .locks
            .lock()
            .entry(guild.to_string())
            .or_default()
            .clone();
        lock.lock_owned().await
    }

    fn guild(&self, guild: &str) -> CellResult<GuildRecord> {
        self.store
            .get_value(GUILDS, &guild_key(guild))
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)
    }

    fn member(&self, guild: &str, user: &str) -> CellResult<Member> {
        self.store
            .get_value(MEMBERS, &member_key(guild, user))
            .map_err(storage)?
            .ok_or(MessageCellError::Forbidden)
    }

    fn channel(&self, guild: &str, channel: &str) -> CellResult<Channel> {
        self.store
            .get_value(CHANNELS, &channel_key(guild, channel))
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)
    }

    fn message(&self, guild: &str, channel: &str, id: &str) -> CellResult<Message> {
        let id = id.parse::<u64>().map_err(|_| MessageCellError::NotFound)?;
        self.store
            .get_value(MESSAGES, &message_key(guild, channel, id))
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)
    }

    /// The guild must exist and the user must hold `permission` in it.
    fn authorize(&self, guild: &str, user: &str, permission: Permission) -> CellResult<Member> {
        self.guild(guild)?;
        let member = self.member(guild, user)?;
        match (permission, member.role) {
            (Permission::ManageGuild, Role::Member) => Err(MessageCellError::Forbidden),
            _ => Ok(member),
        }
    }

    /// Appends `event` to the guild's log (and, when other services mirror
    /// it, to the outbox) as part of `batch`.
    fn append(
        &self,
        batch: &mut Batch,
        record: &mut GuildRecord,
        mut event: GuildEvent,
        forward: bool,
    ) -> i64 {
        record.last_seq += 1;
        event.seq = record.last_seq;
        let guild = record.guild.id.clone();
        batch.put_value(LOG, log_key(&guild, event.seq), &event);
        if forward {
            let seq = self.next_outbox.fetch_add(1, Ordering::Relaxed);
            batch.put_value(
                OUTBOX,
                outbox_key(seq),
                &OutboxEntry {
                    guild,
                    seq: event.seq,
                },
            );
        }
        event.seq
    }

    /// Writes a batch built with [`Self::append`] and wakes its readers.
    fn commit(&self, batch: Batch, record: &GuildRecord) -> CellResult<()> {
        let mut batch = batch;
        batch.put_value(GUILDS, guild_key(&record.guild.id), record);
        self.store.write(batch).map_err(storage)?;
        self.signals.publish(&record.guild.id, record.last_seq);
        self.outbox_signal.notify_one();
        Ok(())
    }

    fn snapshot(&self, record: &GuildRecord) -> CellResult<GuildSnapshot> {
        let prefix = guild_key(&record.guild.id);
        let channels = self
            .store
            .scan_values::<Channel>(Scan::prefix(CHANNELS, &prefix))
            .map_err(storage)?;
        let members = self
            .store
            .scan_values::<Member>(Scan::prefix(MEMBERS, &prefix))
            .map_err(storage)?;
        Ok(GuildSnapshot {
            guild: record.guild.clone(),
            channels: channels.into_iter().map(|(_, channel)| channel).collect(),
            members: members.into_iter().map(|(_, member)| member).collect(),
            last_seq: record.last_seq,
        })
    }

    async fn create_guild(&self, new: NewGuild) -> CellResult<Guild> {
        check_text(&new.name, MAX_NAME_CHARS)?;
        if let Some(description) = &new.description {
            check_text(description, MAX_TOPIC_CHARS)?;
        }
        if !self.translation_cells.contains_key(&new.translation_cell) {
            return Err(MessageCellError::InvalidRequest);
        }
        let _guard = self.lock(&new.guild_id).await;

        // Idempotent, so the Directory can retry a create that timed out.
        if let Ok(record) = self.guild(&new.guild_id) {
            return Ok(record.guild);
        }

        let now = now_ms();
        let guild = Guild {
            id: new.guild_id,
            name: new.name,
            description: new.description,
            owner_id: new.owner_id.clone(),
            created_at: now,
        };
        let owner = Member {
            user_id: new.owner_id,
            name: new.owner_name,
            role: Role::Owner,
            joined_at: now,
        };
        let mut record = GuildRecord {
            guild: guild.clone(),
            translation_cell: new.translation_cell,
            last_seq: 0,
            last_message_id: 0,
        };

        let mut batch = Batch::new();
        batch.put_value(MEMBERS, member_key(&guild.id, &owner.user_id), &owner);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                guild: Some(guild.clone()),
                ..event(GuildEventKind::GuildCreated)
            },
            true,
        );
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                member: Some(owner),
                ..event(GuildEventKind::MemberJoined)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(guild)
    }

    async fn join_guild(
        &self,
        guild: String,
        user: String,
        name: String,
    ) -> CellResult<GuildSnapshot> {
        let _guard = self.lock(&guild).await;
        let mut record = self.guild(&guild)?;
        if self.member(&guild, &user).is_ok() {
            return self.snapshot(&record);
        }

        let member = Member {
            user_id: user.clone(),
            name,
            role: Role::Member,
            joined_at: now_ms(),
        };
        let mut batch = Batch::new();
        batch.put_value(MEMBERS, member_key(&guild, &user), &member);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                member: Some(member),
                ..event(GuildEventKind::MemberJoined)
            },
            true,
        );
        self.commit(batch, &record)?;
        self.snapshot(&record)
    }

    async fn create_channel(&self, new: NewChannel) -> CellResult<Channel> {
        check_text(&new.name, MAX_NAME_CHARS)?;
        if let Some(topic) = &new.topic {
            check_text(topic, MAX_TOPIC_CHARS)?;
        }
        // Anyone may start a thread; categories and channels need managers.
        let permission = match new.kind {
            ChannelKind::Thread => Permission::SendMessages,
            _ => Permission::ManageGuild,
        };
        let _guard = self.lock(&new.guild_id).await;
        self.authorize(&new.guild_id, &new.user_id, permission)?;
        let mut record = self.guild(&new.guild_id)?;

        let parent = new
            .parent_id
            .as_ref()
            .map(|parent| self.channel(&new.guild_id, parent))
            .transpose()?;
        let valid_parent = match (new.kind, parent.as_ref().map(|parent| parent.kind)) {
            (ChannelKind::Category, None) => true,
            (ChannelKind::Text, None | Some(ChannelKind::Category)) => true,
            (ChannelKind::Thread, Some(ChannelKind::Text)) => true,
            _ => false,
        };
        if !valid_parent {
            return Err(MessageCellError::InvalidRequest);
        }
        if let (Some(parent), Some(starter)) = (&parent, &new.starter_message_id) {
            self.message(&new.guild_id, &parent.id, starter)?;
        }

        let channel = Channel {
            id: random_id(),
            guild_id: new.guild_id.clone(),
            kind: new.kind,
            name: new.name,
            topic: new.topic,
            parent_id: new.parent_id,
            starter_message_id: new.starter_message_id,
            created_at: now_ms(),
        };
        let mut batch = Batch::new();
        batch.put_value(
            CHANNELS,
            channel_key(&channel.guild_id, &channel.id),
            &channel,
        );
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                channel: Some(channel.clone()),
                ..event(GuildEventKind::ChannelCreated)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(channel)
    }

    async fn send_message(&self, new: NewMessage) -> CellResult<Message> {
        check_text(&new.text, MAX_TEXT_CHARS)?;
        let _guard = self.lock(&new.guild_id).await;
        let member = self.authorize(&new.guild_id, &new.user_id, Permission::SendMessages)?;
        let channel = self.channel(&new.guild_id, &new.channel_id)?;
        if channel.kind == ChannelKind::Category {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut record = self.guild(&new.guild_id)?;

        let now = now_ms();
        let id = TimeOrderedIds::new(record.last_message_id).next(now);
        record.last_message_id = id;
        let message = Message {
            id: id.to_string(),
            guild_id: new.guild_id.clone(),
            channel_id: new.channel_id.clone(),
            author_id: new.user_id,
            author_name: member.name,
            author_locale: new.author_locale,
            text: new.text,
            rev: 1,
            created_at: now,
            edited_at: None,
        };

        let mut batch = Batch::new();
        batch.put_value(
            MESSAGES,
            message_key(&new.guild_id, &new.channel_id, id),
            &message,
        );
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                message: Some(message.clone()),
                ..event(GuildEventKind::MessageCreated)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(message)
    }

    async fn edit_message(&self, edit: MessageEdit) -> CellResult<Message> {
        check_text(&edit.text, MAX_TEXT_CHARS)?;
        let _guard = self.lock(&edit.guild_id).await;
        self.authorize(&edit.guild_id, &edit.user_id, Permission::SendMessages)?;
        let mut message = self.message(&edit.guild_id, &edit.channel_id, &edit.message_id)?;
        if message.author_id != edit.user_id {
            return Err(MessageCellError::Forbidden);
        }
        let mut record = self.guild(&edit.guild_id)?;

        message.text = edit.text;
        message.rev += 1;
        message.edited_at = Some(now_ms());

        let id = message
            .id
            .parse::<u64>()
            .map_err(|_| MessageCellError::NotFound)?;
        let mut batch = Batch::new();
        batch.put_value(
            MESSAGES,
            message_key(&edit.guild_id, &edit.channel_id, id),
            &message,
        );
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                message: Some(message.clone()),
                ..event(GuildEventKind::MessageEdited)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(message)
    }

    async fn delete_message(
        &self,
        guild: String,
        channel: String,
        message_id: String,
        user: String,
    ) -> CellResult<()> {
        let _guard = self.lock(&guild).await;
        let member = self.authorize(&guild, &user, Permission::View)?;
        let message = self.message(&guild, &channel, &message_id)?;
        if message.author_id != user && member.role != Role::Owner {
            return Err(MessageCellError::Forbidden);
        }
        let mut record = self.guild(&guild)?;

        let id = message
            .id
            .parse::<u64>()
            .map_err(|_| MessageCellError::NotFound)?;
        let mut batch = Batch::new();
        batch.delete(MESSAGES, message_key(&guild, &channel, id));
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                deleted: Some(DeletedMessage {
                    channel_id: channel,
                    message_id,
                }),
                ..event(GuildEventKind::MessageDeleted)
            },
            true,
        );
        self.commit(batch, &record)
    }

    fn get_messages(
        &self,
        guild: String,
        channel: String,
        user: String,
        before: Option<String>,
        limit: i64,
    ) -> CellResult<Vec<Message>> {
        self.authorize(&guild, &user, Permission::View)?;
        self.channel(&guild, &channel)?;

        let prefix = message_prefix(&guild, &channel);
        let before = before
            .map(|id| {
                id.parse::<u64>()
                    .map_err(|_| MessageCellError::InvalidRequest)
            })
            .transpose()?
            .map(|id| message_key(&guild, &channel, id));
        let mut scan = Scan::prefix(MESSAGES, &prefix)
            .reverse()
            .limit(limit.clamp(1, MAX_PAGE) as usize);
        if let Some(before) = &before {
            scan = scan.after(before);
        }

        Ok(self
            .store
            .scan_values::<Message>(scan)
            .map_err(storage)?
            .into_iter()
            .map(|(_, message)| message)
            .collect())
    }

    async fn poll_events(
        &self,
        guild: String,
        after: i64,
        wait_ms: i64,
    ) -> CellResult<GuildEventBatch> {
        let head = self.guild(&guild)?.last_seq;
        let prefix = guild_key(&guild);
        let after_key = log_key(&guild, after.max(0));
        let read = || {
            self.store
                .scan_values::<GuildEvent>(
                    Scan::prefix(LOG, &prefix)
                        .after(&after_key)
                        .limit(POLL_LIMIT),
                )
                .map_err(storage)
        };

        let mut events = read()?;
        if events.is_empty()
            && self
                .signals
                .wait_past(&guild, after, poll_timeout(wait_ms))
                .await
        {
            events = read()?;
        }

        let events = events
            .into_iter()
            .map(|(_, event)| event)
            .collect::<Vec<_>>();
        Ok(GuildEventBatch {
            next: events
                .last()
                .map(|event| event.seq)
                .unwrap_or(after.min(head)),
            head: head.max(events.last().map(|event| event.seq).unwrap_or(0)),
            events,
        })
    }
}

#[async_trait]
impl babel_api_message_cell::types::MessageCell for MessageCell {
    async fn create_guild(&self, body: NewGuild) -> CellResult<Guild> {
        self.inner.create_guild(body).await
    }

    async fn get_guild(&self, guild_id: String, user_id: String) -> CellResult<GuildSnapshot> {
        self.inner
            .authorize(&guild_id, &user_id, Permission::View)?;
        self.inner.snapshot(&self.inner.guild(&guild_id)?)
    }

    async fn join_guild(
        &self,
        guild_id: String,
        user_id: String,
        user_name: String,
    ) -> CellResult<GuildSnapshot> {
        self.inner.join_guild(guild_id, user_id, user_name).await
    }

    async fn check_permission(
        &self,
        guild_id: String,
        user_id: String,
        permission: Permission,
    ) -> CellResult<()> {
        self.inner
            .authorize(&guild_id, &user_id, permission)
            .map(|_| ())
    }

    async fn create_channel(&self, body: NewChannel) -> CellResult<Channel> {
        self.inner.create_channel(body).await
    }

    async fn send_message(&self, body: NewMessage) -> CellResult<Message> {
        self.inner.send_message(body).await
    }

    async fn edit_message(&self, body: MessageEdit) -> CellResult<Message> {
        self.inner.edit_message(body).await
    }

    async fn delete_message(
        &self,
        guild_id: String,
        channel_id: String,
        message_id: String,
        user_id: String,
    ) -> CellResult<()> {
        self.inner
            .delete_message(guild_id, channel_id, message_id, user_id)
            .await
    }

    async fn get_messages(
        &self,
        guild_id: String,
        channel_id: String,
        user_id: String,
        before: Option<String>,
        limit: i64,
    ) -> CellResult<Vec<Message>> {
        self.inner
            .get_messages(guild_id, channel_id, user_id, before, limit)
    }

    async fn poll_events(
        &self,
        guild_id: String,
        after: i64,
        wait_ms: i64,
    ) -> CellResult<GuildEventBatch> {
        self.inner.poll_events(guild_id, after, wait_ms).await
    }
}
