//! The Message Cell: guilds, channels, members and original messages.
//!
//! One process holds many guilds. Each guild has a single writer (here, a
//! per-guild lock) and an event log. Every change is one atomic batch:
//!
//! ```text
//! state change + log entry (+ outbox entry)
//!        │            │              └─▶ outbox worker ─▶ Translation Cell, Directory, Media
//!        │            └─▶ poll_events ─▶ Edges ─▶ clients
//!        └─▶ reads (get_guild, get_messages)
//! ```
//!
//! See docs/architecture/message-cell.md.

mod moving;
mod outbox;
mod records;
mod trim;

use std::{
    collections::HashMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

use async_trait::async_trait;
use babel_api_directory::types::Directory;
use babel_api_media::types::Media;
use babel_api_message_cell::types::{
    Attachment, Ban, Channel, ChannelKind, ChannelUpdate, DeletedMessage, Emoji, Guild, GuildEvent,
    GuildEventBatch, GuildEventKind, GuildExport, GuildImport, GuildSnapshot, GuildUpdate, Invite,
    Member, Message, MessageCellError, MessageEdit, NewChannel, NewEmoji, NewGuild, NewInvite,
    NewMessage, NewUpload, NotificationSettings, NotifyLevel, Permission, Reaction, ReactionChange,
    ReadState, Role, Typing,
};
use babel_api_translation_cell::types::TranslationCell;
use babel_common::{
    LogMarks, LogSignals, TimeOrderedIds, id_floor, now_ms, poll_timeout, random_id,
};
use babel_store::{Batch, KvStore, Scan, StoreError};
use parking_lot::Mutex;
pub use records::TABLES;
use records::{
    BANS, CHANNELS, EMOJI, GUILD_TABLES, GUILDS, GuildRecord, INVITES, LOG, MEMBERS, MESSAGES,
    NOTIFY, OUTBOX, OutboxEntry, READS, UPLOADS, Upload, channel_key, emoji_key, guild_key,
    invite_key, log_key, member_key, message_key, message_prefix, outbox_key, read_key,
    read_prefix, upload_key, upload_prefix,
};
use tokio::sync::{Notify, OwnedMutexGuard};
pub use trim::{LOG_RETENTION, UPLOADS_KEPT};

const MAX_NAME_CHARS: usize = 100;
const MAX_TOPIC_CHARS: usize = 1000;
const MAX_TEXT_CHARS: usize = 4000;
const MAX_PAGE: i64 = 100;
const POLL_LIMIT: usize = 100;
const MAX_MENTIONS: usize = 20;
const MAX_CHANNEL_SETTINGS: usize = 500;
/// Different emoji on one message.
const MAX_REACTIONS: usize = 20;
/// Files sent with one message.
const MAX_ATTACHMENTS: usize = 10;
/// Files a member may have uploaded and not sent yet, per guild.
const MAX_UPLOADS: usize = 50;
/// A guild's own emoji.
const MAX_EMOJI: usize = 50;
/// Messages read when recounting a member's mentions after an ack.
const RECOUNT_LIMIT: usize = 1000;

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
        reaction: None,
        emoji: None,
        typing: None,
        user_id: None,
        read_state: None,
        notifications: None,
    }
}

fn check_text(text: &str, max: usize) -> CellResult<()> {
    let len = text.chars().count();
    if text.trim().is_empty() || len > max {
        return Err(MessageCellError::InvalidRequest);
    }
    Ok(())
}

/// A message's text, which may be empty when files go with it.
fn check_message_text(text: &str, attachments: usize) -> CellResult<()> {
    if attachments > 0 && text.trim().is_empty() {
        return Ok(());
    }
    check_text(text, MAX_TEXT_CHARS)
}

/// Members named with `@name` in `text`. Names are unique and hold no
/// whitespace, so `@` starts a name wherever it does not follow a letter or
/// digit (`mail@example` names no one). The longest matching name wins, and
/// must end where the word does: `@bobby` is not `@bob`, `@bob,` is.
fn find_mentions<'a>(text: &str, members: &'a [Member]) -> Vec<&'a Member> {
    let lowered = members
        .iter()
        .map(|member| (member.name.to_lowercase(), member))
        .collect::<Vec<_>>();
    let mut found = Vec::<&Member>::new();
    let mut previous = None::<char>;
    for (at, c) in text.char_indices() {
        let starts = c == '@' && previous.is_none_or(|p| !p.is_alphanumeric());
        previous = Some(c);
        if !starts {
            continue;
        }
        let word = text[at + 1..]
            .split(char::is_whitespace)
            .next()
            .unwrap_or_default()
            .to_lowercase();
        let best = lowered
            .iter()
            .filter(|(name, _)| {
                word.strip_prefix(name.as_str()).is_some_and(|rest| {
                    rest.chars()
                        .next()
                        .is_none_or(|next| !next.is_alphanumeric() && next != '_')
                })
            })
            .max_by_key(|(name, _)| name.len());
        if let Some((_, member)) = best
            && found.iter().all(|known| known.user_id != member.user_id)
        {
            found.push(member);
        }
    }
    found
}

/// Who a message mentions; messages stored before mentions existed
/// mention no one.
fn mentions(message: &Message) -> &[String] {
    message.mentions.as_deref().unwrap_or_default()
}

fn parse_id(id: &str) -> CellResult<u64> {
    id.parse().map_err(|_| MessageCellError::InvalidRequest)
}

/// One Unicode emoji, in its fully qualified form (`❤` becomes `❤️`), so
/// the same emoji is always one reaction; or the code of a guild's emoji,
/// which the Edge found the user may use.
fn emoji(text: &str) -> CellResult<String> {
    if let Some(custom) = babel_emoji::parse(text) {
        return Ok(custom.code());
    }
    emojis::get(text)
        .map(|emoji| emoji.as_str().to_string())
        .ok_or(MessageCellError::InvalidRequest)
}

/// The same reaction: a guild's emoji is the same whatever name its code
/// gives it.
fn same_emoji(a: &str, b: &str) -> bool {
    match (babel_emoji::parse(a), babel_emoji::parse(b)) {
        (Some(a), Some(b)) => a.key() == b.key(),
        _ => a == b,
    }
}

/// Owners manage every channel; a thread's creator manages the thread.
fn may_manage(member: &Member, channel: &Channel) -> CellResult<()> {
    let creator = channel.kind == ChannelKind::Thread
        && channel.created_by.as_deref() == Some(member.user_id.as_str());
    if member.role == Role::Owner || creator {
        Ok(())
    } else {
        Err(MessageCellError::Forbidden)
    }
}

#[derive(Clone)]
pub struct MessageCell {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) store: Arc<dyn KvStore>,
    /// Translation Cells by name; each guild records which one it uses.
    pub(crate) translation_cells: HashMap<String, Arc<dyn TranslationCell>>,
    /// Media services by name; an attachment's ID begins with the one
    /// holding it.
    pub(crate) media: HashMap<String, Arc<dyn Media>>,
    /// Set once the Directory exists (it also needs the Message Cells).
    pub(crate) directory: OnceLock<Arc<dyn Directory>>,
    pub(crate) outbox_signal: Notify,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    signals: LogSignals<String>,
    /// How far each guild's log had grown at each trimming pass.
    marks: LogMarks<String>,
    /// Bumped whenever a move freezes or releases a guild; frozen writers
    /// wait for it.
    moves: tokio::sync::watch::Sender<u64>,
    next_outbox: AtomicI64,
}

impl MessageCell {
    /// Opens the cell and starts its outbox worker. Must be called within a
    /// Tokio runtime.
    pub fn open(
        store: Arc<dyn KvStore>,
        translation_cells: HashMap<String, Arc<dyn TranslationCell>>,
        media: HashMap<String, Arc<dyn Media>>,
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
            media,
            directory: OnceLock::new(),
            outbox_signal: Notify::new(),
            locks: Mutex::default(),
            signals: LogSignals::new(),
            marks: LogMarks::default(),
            moves: tokio::sync::watch::channel(0).0,
            next_outbox: AtomicI64::new(next_outbox),
        });
        tokio::spawn(outbox::run(inner.clone()));
        tokio::spawn(trim::run(inner.clone()));
        Ok(Self { inner })
    }

    /// Gives the outbox somewhere to send membership changes.
    pub fn connect_directory(&self, directory: Arc<dyn Directory>) {
        let _ = self.inner.directory.set(directory);
        self.inner.outbox_signal.notify_one();
    }

    /// Trims every guild's log to `retention` now (it also happens every
    /// few minutes, to [`LOG_RETENTION`]). Returns how many entries went.
    pub async fn trim_logs(&self, retention: std::time::Duration) -> Result<usize, StoreError> {
        self.inner.trim_logs(retention).await
    }

    /// Forgets uploads nobody sent within `kept` now (it also happens every
    /// few minutes, after [`UPLOADS_KEPT`]). Returns how many went.
    pub fn expire_uploads(&self, kept: std::time::Duration) -> Result<usize, StoreError> {
        self.inner.expire_uploads(kept)
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

    /// The guild's record; `moved` once it lives on another cell.
    fn guild(&self, guild: &str) -> CellResult<GuildRecord> {
        let record = self.guild_log(guild)?;
        if record.deleted {
            return Err(MessageCellError::NotFound);
        }
        Ok(record)
    }

    /// The guild's record while its log may still be read: deleted guilds
    /// too.
    fn guild_log(&self, guild: &str) -> CellResult<GuildRecord> {
        let record = self
            .store
            .get_value::<GuildRecord>(GUILDS, &guild_key(guild))
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)?;
        if record.moved {
            return Err(MessageCellError::Moved);
        }
        Ok(record)
    }

    /// Takes the guild's write lock once it accepts writes. While the guild
    /// is being moved, writers wait here; once it has moved, they learn so.
    async fn lock_writable(&self, guild: &str) -> CellResult<OwnedMutexGuard<()>> {
        let deadline = tokio::time::Instant::now() + moving::WRITE_WAIT;
        loop {
            let mut moves = self.moves.subscribe();
            let guard = self.lock(guild).await;
            if !self.guild(guild)?.frozen {
                return Ok(guard);
            }
            drop(guard);
            if tokio::time::timeout_at(deadline, moves.changed())
                .await
                .is_err()
            {
                return Err(MessageCellError::Unavailable);
            }
        }
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
        // Members may invite; until roles grow, only owners manage.
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

    /// A member's read state in a channel; before any, everything since
    /// they joined is unread.
    fn read_state(&self, guild: &str, member: &Member, channel: &str) -> CellResult<ReadState> {
        Ok(self
            .store
            .get_value(READS, &read_key(guild, &member.user_id, channel))
            .map_err(storage)?
            .unwrap_or_else(|| ReadState {
                channel_id: channel.to_string(),
                last_read_id: id_floor(member.joined_at).to_string(),
                mention_count: 0,
            }))
    }

    fn notifications(&self, guild: &str, user: &str) -> CellResult<NotificationSettings> {
        Ok(self
            .store
            .get_value(NOTIFY, &member_key(guild, user))
            .map_err(storage)?
            .unwrap_or_else(|| NotificationSettings {
                guild_id: guild.to_string(),
                level: NotifyLevel::All,
                channels: Vec::new(),
            }))
    }

    /// Who a message notifies: members named in `text` and the author of
    /// the message it replies to, never its own author.
    fn mentioned(
        &self,
        guild: &str,
        text: &str,
        reply_author: Option<&str>,
        author: &str,
    ) -> CellResult<Vec<Member>> {
        let mut mentioned = Vec::new();
        if text.contains('@') {
            let members = self
                .store
                .scan_values::<Member>(Scan::prefix(MEMBERS, &guild_key(guild)))
                .map_err(storage)?
                .into_iter()
                .map(|(_, member)| member)
                .collect::<Vec<_>>();
            mentioned.extend(find_mentions(text, &members).into_iter().cloned());
        }
        if let Some(user) = reply_author
            && mentioned.iter().all(|member| member.user_id != user)
            && let Ok(member) = self.member(guild, user)
        {
            mentioned.push(member);
        }
        mentioned.retain(|member| member.user_id != author);
        mentioned.truncate(MAX_MENTIONS);
        Ok(mentioned)
    }

    /// Members of `ids` that are still in the guild.
    fn members_of(&self, guild: &str, ids: &[String]) -> Vec<Member> {
        ids.iter()
            .filter_map(|id| self.member(guild, id).ok())
            .collect()
    }

    /// Adds `delta` to the mention counts of members who have not read
    /// message `id` yet.
    fn count_mentions(
        &self,
        batch: &mut Batch,
        guild: &str,
        channel: &str,
        id: u64,
        members: &[Member],
        delta: i64,
    ) -> CellResult<()> {
        for member in members {
            let mut state = self.read_state(guild, member, channel)?;
            if state.last_read_id.parse::<u64>().unwrap_or(0) >= id {
                continue;
            }
            state.mention_count = (state.mention_count + delta).max(0);
            batch.put_value(READS, read_key(guild, &member.user_id, channel), &state);
        }
        Ok(())
    }

    /// Removes a member and everything kept for them, as part of `batch`.
    fn drop_member(
        &self,
        batch: &mut Batch,
        record: &mut GuildRecord,
        member: Member,
        kind: GuildEventKind,
    ) {
        let guild = record.guild.id.clone();
        batch.delete(MEMBERS, member_key(&guild, &member.user_id));
        batch.delete(NOTIFY, member_key(&guild, &member.user_id));
        batch.delete_prefix(READS, read_prefix(&guild, &member.user_id));
        self.append(
            batch,
            record,
            GuildEvent {
                member: Some(member),
                ..event(kind)
            },
            true,
        );
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

    /// The guild as `user`, a member, sees it.
    fn snapshot(&self, record: &GuildRecord, user: &str) -> CellResult<GuildSnapshot> {
        let guild = &record.guild.id;
        let prefix = guild_key(guild);
        let channels = self
            .store
            .scan_values::<Channel>(Scan::prefix(CHANNELS, &prefix))
            .map_err(storage)?
            .into_iter()
            .map(|(_, channel)| channel)
            .collect::<Vec<_>>();
        let members = self
            .store
            .scan_values::<Member>(Scan::prefix(MEMBERS, &prefix))
            .map_err(storage)?
            .into_iter()
            .map(|(_, member)| member)
            .collect::<Vec<_>>();
        let member = members
            .iter()
            .find(|member| member.user_id == user)
            .ok_or(MessageCellError::Forbidden)?;
        let mut stored = self
            .store
            .scan_values::<ReadState>(Scan::prefix(READS, &read_prefix(guild, user)))
            .map_err(storage)?
            .into_iter()
            .map(|(_, state)| (state.channel_id.clone(), state))
            .collect::<HashMap<_, _>>();
        let read_states = channels
            .iter()
            .filter(|channel| channel.kind != ChannelKind::Category)
            .map(|channel| {
                stored.remove(&channel.id).unwrap_or_else(|| ReadState {
                    channel_id: channel.id.clone(),
                    last_read_id: id_floor(member.joined_at).to_string(),
                    mention_count: 0,
                })
            })
            .collect();
        Ok(GuildSnapshot {
            guild: record.guild.clone(),
            notifications: self.notifications(guild, user)?,
            emoji: self.emoji_of(guild)?,
            read_states,
            channels,
            members,
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
            trimmed: 0,
            frozen: false,
            moved: false,
            deleted: false,
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

    async fn update_guild(&self, update: GuildUpdate) -> CellResult<Guild> {
        check_text(&update.name, MAX_NAME_CHARS)?;
        if let Some(description) = &update.description {
            check_text(description, MAX_TOPIC_CHARS)?;
        }
        let _guard = self.lock_writable(&update.guild_id).await?;
        self.authorize(&update.guild_id, &update.user_id, Permission::ManageGuild)?;
        let mut record = self.guild(&update.guild_id)?;
        record.guild.name = update.name;
        record.guild.description = update.description;

        let guild = record.guild.clone();
        let mut batch = Batch::new();
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                guild: Some(guild.clone()),
                ..event(GuildEventKind::GuildUpdated)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(guild)
    }

    /// Drops everything but the record and the log, which tells Edges and
    /// the outbox: the guild went, then each member, so the Directory
    /// forgets their memberships.
    async fn delete_guild(&self, guild: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        if member.role != Role::Owner {
            return Err(MessageCellError::Forbidden);
        }
        let mut record = self.guild(&guild)?;
        let prefix = guild_key(&guild);
        let members = self
            .store
            .scan_values::<Member>(Scan::prefix(MEMBERS, &prefix))
            .map_err(storage)?;

        let mut batch = Batch::new();
        for table in GUILD_TABLES
            .iter()
            .copied()
            .filter(|table| ![LOG, GUILDS].contains(table))
        {
            batch.delete_prefix(table, prefix.clone());
        }
        self.append(
            &mut batch,
            &mut record,
            event(GuildEventKind::GuildDeleted),
            true,
        );
        for (_, member) in members {
            self.append(
                &mut batch,
                &mut record,
                GuildEvent {
                    member: Some(member),
                    ..event(GuildEventKind::MemberRemoved)
                },
                true,
            );
        }
        record.deleted = true;
        self.commit(batch, &record)
    }

    async fn join_guild(
        &self,
        guild: String,
        invite: String,
        user: String,
        name: String,
    ) -> CellResult<GuildSnapshot> {
        let _guard = self.lock_writable(&guild).await?;
        let mut record = self.guild(&guild)?;
        if self.member(&guild, &user).is_ok() {
            return self.snapshot(&record, &user);
        }
        let banned = self
            .store
            .get(BANS, &member_key(&guild, &user))
            .map_err(storage)?
            .is_some();

        // Unknown, expired and used-up invites, and bans, look alike.
        let now = now_ms();
        let mut invite = self
            .store
            .get_value::<Invite>(INVITES, &invite_key(&guild, &invite))
            .map_err(storage)?
            .filter(|invite| invite.expires_at.is_none_or(|at| now < at))
            .filter(|invite| invite.max_uses.is_none_or(|max| invite.uses < max))
            .filter(|_| !banned)
            .ok_or(MessageCellError::NotFound)?;
        invite.uses += 1;

        let member = Member {
            user_id: user.clone(),
            name,
            role: Role::Member,
            joined_at: now,
        };
        let mut batch = Batch::new();
        batch.put_value(INVITES, invite_key(&guild, &invite.secret), &invite);
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
        self.snapshot(&record, &user)
    }

    async fn create_invite(&self, new: NewInvite) -> CellResult<Invite> {
        let _guard = self.lock_writable(&new.guild_id).await?;
        self.authorize(&new.guild_id, &new.user_id, Permission::CreateInvite)?;
        if new.max_uses.is_some_and(|max| max < 1) {
            return Err(MessageCellError::InvalidRequest);
        }
        let invite = Invite {
            secret: random_id(),
            guild_id: new.guild_id,
            created_by: new.user_id,
            created_at: now_ms(),
            expires_at: new.expires_at,
            max_uses: new.max_uses,
            uses: 0,
        };
        let mut batch = Batch::new();
        batch.put_value(
            INVITES,
            invite_key(&invite.guild_id, &invite.secret),
            &invite,
        );
        self.store.write(batch).map_err(storage)?;
        Ok(invite)
    }

    async fn revoke_invite(&self, guild: String, secret: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        let key = invite_key(&guild, &secret);
        let invite = self
            .store
            .get_value::<Invite>(INVITES, &key)
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)?;
        if invite.created_by != user && member.role != Role::Owner {
            return Err(MessageCellError::Forbidden);
        }
        let mut batch = Batch::new();
        batch.delete(INVITES, key);
        self.store.write(batch).map_err(storage)
    }

    /// Valid invites the user may see; expired and used-up ones are
    /// deleted on the way.
    async fn get_invites(&self, guild: String, user: String) -> CellResult<Vec<Invite>> {
        let member = self.authorize(&guild, &user, Permission::CreateInvite)?;
        let now = now_ms();
        let mut stale = Batch::new();
        let mut invites = Vec::new();
        for (key, invite) in self
            .store
            .scan_values::<Invite>(Scan::prefix(INVITES, &guild_key(&guild)))
            .map_err(storage)?
        {
            let valid = invite.expires_at.is_none_or(|at| now < at)
                && invite.max_uses.is_none_or(|max| invite.uses < max);
            if !valid {
                stale.delete(INVITES, key);
            } else if member.role == Role::Owner || invite.created_by == user {
                invites.push(invite);
            }
        }
        if !stale.is_empty() {
            self.store.write(stale).map_err(storage)?;
        }
        invites.sort_by_key(|invite| invite.created_at);
        Ok(invites)
    }

    async fn leave_guild(&self, guild: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        if member.role == Role::Owner {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        self.drop_member(&mut batch, &mut record, member, GuildEventKind::MemberLeft);
        self.commit(batch, &record)
    }

    async fn remove_member(
        &self,
        guild: String,
        user: String,
        target: String,
        ban: bool,
    ) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &user, Permission::ManageGuild)?;
        let member = self
            .member(&guild, &target)
            .map_err(|_| MessageCellError::NotFound)?;
        if member.role == Role::Owner {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        if ban {
            batch.put_value(
                BANS,
                member_key(&guild, &target),
                &Ban {
                    user_id: target.clone(),
                    name: member.name.clone(),
                    banned_by: user,
                    banned_at: now_ms(),
                },
            );
        }
        self.drop_member(
            &mut batch,
            &mut record,
            member,
            GuildEventKind::MemberRemoved,
        );
        self.commit(batch, &record)
    }

    async fn unban_member(&self, guild: String, user: String, target: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &user, Permission::ManageGuild)?;
        let key = member_key(&guild, &target);
        if self.store.get(BANS, &key).map_err(storage)?.is_none() {
            return Err(MessageCellError::NotFound);
        }
        let mut batch = Batch::new();
        batch.delete(BANS, key);
        self.store.write(batch).map_err(storage)
    }

    fn get_bans(&self, guild: String, user: String) -> CellResult<Vec<Ban>> {
        self.authorize(&guild, &user, Permission::ManageGuild)?;
        Ok(self
            .store
            .scan_values::<Ban>(Scan::prefix(BANS, &guild_key(&guild)))
            .map_err(storage)?
            .into_iter()
            .map(|(_, ban)| ban)
            .collect())
    }

    async fn ack(
        &self,
        guild: String,
        channel: String,
        user: String,
        message_id: String,
    ) -> CellResult<ReadState> {
        let target = parse_id(&message_id)?;
        // Serialized with sends, which count mentions into the same state.
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        let newest = self
            .channel(&guild, &channel)?
            .last_message_id
            .and_then(|id| id.parse::<u64>().ok())
            .unwrap_or(0);
        let mut state = self.read_state(&guild, &member, &channel)?;
        let read = target
            .min(newest)
            .max(state.last_read_id.parse().unwrap_or(0));

        let prefix = message_prefix(&guild, &channel);
        let after = message_key(&guild, &channel, read);
        state.mention_count = self
            .store
            .scan_values::<Message>(
                Scan::prefix(MESSAGES, &prefix)
                    .after(&after)
                    .limit(RECOUNT_LIMIT),
            )
            .map_err(storage)?
            .iter()
            .filter(|(_, message)| mentions(message).contains(&user))
            .count() as i64;
        state.last_read_id = read.to_string();

        // The log carries it to the member's other sessions, on any Edge.
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.put_value(READS, read_key(&guild, &user, &channel), &state);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                user_id: Some(user),
                read_state: Some(state.clone()),
                ..event(GuildEventKind::ReadStateUpdated)
            },
            false,
        );
        self.commit(batch, &record)?;
        Ok(state)
    }

    async fn put_notification_settings(
        &self,
        user: String,
        mut settings: NotificationSettings,
    ) -> CellResult<NotificationSettings> {
        let guild = settings.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &user, Permission::View)?;
        // The last entry for a channel wins; unknown channels are dropped.
        let mut seen = std::collections::HashSet::new();
        settings.channels.reverse();
        settings.channels.retain(|entry| {
            seen.insert(entry.channel_id.clone()) && self.channel(&guild, &entry.channel_id).is_ok()
        });
        settings.channels.reverse();
        if settings.channels.len() > MAX_CHANNEL_SETTINGS {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.put_value(NOTIFY, member_key(&guild, &user), &settings);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                user_id: Some(user),
                notifications: Some(settings.clone()),
                ..event(GuildEventKind::NotificationsUpdated)
            },
            false,
        );
        self.commit(batch, &record)?;
        Ok(settings)
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
        let _guard = self.lock_writable(&new.guild_id).await?;
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
            last_message_id: None,
            created_by: Some(new.user_id.clone()),
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

    async fn update_channel(&self, update: ChannelUpdate) -> CellResult<Channel> {
        check_text(&update.name, MAX_NAME_CHARS)?;
        if let Some(topic) = &update.topic {
            check_text(topic, MAX_TOPIC_CHARS)?;
        }
        let guild = update.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &update.user_id, Permission::View)?;
        let mut channel = self.channel(&guild, &update.channel_id)?;
        may_manage(&member, &channel)?;
        channel.name = update.name;
        channel.topic = update.topic;

        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.put_value(CHANNELS, channel_key(&guild, &channel.id), &channel);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                channel: Some(channel.clone()),
                ..event(GuildEventKind::ChannelUpdated)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(channel)
    }

    /// Deletes a channel with its messages and members' read markers; a text
    /// channel's threads go first. A category's channels move out of it.
    async fn delete_channel(&self, guild: String, channel: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        let channel = self.channel(&guild, &channel)?;
        may_manage(&member, &channel)?;
        let mut record = self.guild(&guild)?;
        let prefix = guild_key(&guild);
        let children = self
            .store
            .scan_values::<Channel>(Scan::prefix(CHANNELS, &prefix))
            .map_err(storage)?
            .into_iter()
            .map(|(_, child)| child)
            .filter(|child| child.parent_id.as_ref() == Some(&channel.id))
            .collect::<Vec<_>>();

        let mut batch = Batch::new();
        let mut gone = Vec::new();
        for mut child in children {
            if channel.kind == ChannelKind::Category {
                child.parent_id = None;
                batch.put_value(CHANNELS, channel_key(&guild, &child.id), &child);
                self.append(
                    &mut batch,
                    &mut record,
                    GuildEvent {
                        channel: Some(child),
                        ..event(GuildEventKind::ChannelUpdated)
                    },
                    true,
                );
            } else {
                gone.push(child);
            }
        }
        gone.push(channel);

        let ids = gone
            .iter()
            .map(|channel| channel.id.as_str())
            .collect::<Vec<_>>();
        for (key, state) in self
            .store
            .scan_values::<ReadState>(Scan::prefix(READS, &prefix))
            .map_err(storage)?
        {
            if ids.contains(&state.channel_id.as_str()) {
                batch.delete(READS, key);
            }
        }
        for channel in gone {
            batch.delete(CHANNELS, channel_key(&guild, &channel.id));
            batch.delete_prefix(MESSAGES, message_prefix(&guild, &channel.id));
            self.append(
                &mut batch,
                &mut record,
                GuildEvent {
                    channel: Some(channel),
                    ..event(GuildEventKind::ChannelDeleted)
                },
                true,
            );
        }
        self.commit(batch, &record)
    }

    async fn add_upload(&self, new: NewUpload) -> CellResult<()> {
        let guild = new.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &new.user_id, Permission::SendMessages)?;
        let channel = self.channel(&guild, &new.channel_id)?;
        if channel.kind == ChannelKind::Category {
            return Err(MessageCellError::InvalidRequest);
        }
        let pending = self
            .store
            .scan(Scan::prefix(UPLOADS, &upload_prefix(&guild, &new.user_id)).limit(MAX_UPLOADS))
            .map_err(storage)?;
        if pending.len() >= MAX_UPLOADS {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut batch = Batch::new();
        batch.put_value(
            UPLOADS,
            upload_key(&guild, &new.user_id, &new.attachment.id),
            &Upload {
                channel_id: channel.id,
                attachment: new.attachment,
                uploaded_at: now_ms(),
            },
        );
        self.store.write(batch).map_err(storage)
    }

    /// The guild's own emoji, oldest first.
    fn emoji_of(&self, guild: &str) -> CellResult<Vec<Emoji>> {
        let mut emoji = self
            .store
            .scan_values::<Emoji>(Scan::prefix(EMOJI, &guild_key(guild)))
            .map_err(storage)?
            .into_iter()
            .map(|(_, emoji)| emoji)
            .collect::<Vec<_>>();
        emoji.sort_by_key(|emoji| emoji.created_at);
        Ok(emoji)
    }

    async fn create_emoji(&self, new: NewEmoji) -> CellResult<Emoji> {
        if !babel_emoji::valid_name(&new.name) || new.id.is_empty() {
            return Err(MessageCellError::InvalidRequest);
        }
        let guild = new.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &new.user_id, Permission::ManageGuild)?;
        let existing = self.emoji_of(&guild)?;
        // The Edge may retry.
        if let Some(emoji) = existing.iter().find(|emoji| emoji.id == new.id) {
            return Ok(emoji.clone());
        }
        // Names are typed as `:name:`: `Party` and `party` would be one.
        if existing
            .iter()
            .any(|emoji| emoji.name.eq_ignore_ascii_case(&new.name))
        {
            return Err(MessageCellError::NameTaken);
        }
        if existing.len() >= MAX_EMOJI {
            return Err(MessageCellError::LimitExceeded);
        }
        let emoji = Emoji {
            id: new.id,
            guild_id: guild.clone(),
            name: new.name,
            created_by: new.user_id,
            created_at: now_ms(),
        };
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.put_value(EMOJI, emoji_key(&guild, &emoji.id), &emoji);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                emoji: Some(emoji.clone()),
                ..event(GuildEventKind::EmojiCreated)
            },
            true,
        );
        self.commit(batch, &record)?;
        Ok(emoji)
    }

    async fn delete_emoji(&self, guild: String, id: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &user, Permission::ManageGuild)?;
        let key = emoji_key(&guild, &id);
        let emoji = self
            .store
            .get_value::<Emoji>(EMOJI, &key)
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)?;
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.delete(EMOJI, key);
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                emoji: Some(emoji),
                ..event(GuildEventKind::EmojiDeleted)
            },
            true,
        );
        self.commit(batch, &record)
    }

    /// The member's uploads to the channel named by `ids`, which sending
    /// takes: deleted in `batch`.
    fn take_uploads(
        &self,
        batch: &mut Batch,
        guild: &str,
        user: &str,
        channel: &str,
        ids: &[String],
    ) -> CellResult<Vec<Attachment>> {
        if ids.len() > MAX_ATTACHMENTS {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut attachments = Vec::<Attachment>::new();
        for id in ids {
            let key = upload_key(guild, user, id);
            let upload = self
                .store
                .get_value::<Upload>(UPLOADS, &key)
                .map_err(storage)?
                .filter(|upload| upload.channel_id == channel)
                .ok_or(MessageCellError::InvalidRequest)?;
            if attachments.iter().any(|attachment| &attachment.id == id) {
                return Err(MessageCellError::InvalidRequest);
            }
            batch.delete(UPLOADS, key);
            attachments.push(upload.attachment);
        }
        Ok(attachments)
    }

    async fn send_message(&self, new: NewMessage) -> CellResult<Message> {
        let ids = new.attachments.clone().unwrap_or_default();
        check_message_text(&new.text, ids.len())?;
        let guild = new.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &new.user_id, Permission::SendMessages)?;
        let mut channel = self.channel(&guild, &new.channel_id)?;
        if channel.kind == ChannelKind::Category {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut batch = Batch::new();
        let attachments = self.take_uploads(&mut batch, &guild, &new.user_id, &channel.id, &ids)?;
        let reply_author = new
            .reply_to
            .as_ref()
            .map(|reply| self.message(&guild, &channel.id, reply))
            .transpose()?
            .map(|replied| replied.author_id);
        let mentioned = self.mentioned(&guild, &new.text, reply_author.as_deref(), &new.user_id)?;
        let mut record = self.guild(&guild)?;

        let now = now_ms();
        let id = TimeOrderedIds::new(record.last_message_id).next(now);
        record.last_message_id = id;
        let message = Message {
            id: id.to_string(),
            guild_id: guild.clone(),
            channel_id: channel.id.clone(),
            author_id: new.user_id,
            author_name: member.name,
            author_locale: new.author_locale,
            text: new.text,
            reply_to: new.reply_to,
            mentions: Some(
                mentioned
                    .iter()
                    .map(|member| member.user_id.clone())
                    .collect(),
            ),
            rev: 1,
            created_at: now,
            edited_at: None,
            reactions: None,
            attachments: (!attachments.is_empty()).then_some(attachments),
        };
        channel.last_message_id = Some(message.id.clone());

        batch.put_value(MESSAGES, message_key(&guild, &channel.id, id), &message);
        batch.put_value(CHANNELS, channel_key(&guild, &channel.id), &channel);
        // Writing in a channel reads it.
        batch.put_value(
            READS,
            read_key(&guild, &message.author_id, &channel.id),
            &ReadState {
                channel_id: channel.id.clone(),
                last_read_id: message.id.clone(),
                mention_count: 0,
            },
        );
        self.count_mentions(&mut batch, &guild, &channel.id, id, &mentioned, 1)?;
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
        let guild = edit.guild_id.clone();
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &edit.user_id, Permission::SendMessages)?;
        let mut message = self.message(&guild, &edit.channel_id, &edit.message_id)?;
        if message.author_id != edit.user_id {
            return Err(MessageCellError::Forbidden);
        }
        let mut record = self.guild(&guild)?;

        // The replied message may be gone by now; its author was already
        // counted when the reply was sent.
        let reply_author = message
            .reply_to
            .as_ref()
            .and_then(|reply| self.message(&guild, &message.channel_id, reply).ok())
            .map(|replied| replied.author_id);
        let mentioned = self.mentioned(
            &guild,
            &edit.text,
            reply_author.as_deref(),
            &message.author_id,
        )?;
        let now = mentioned
            .iter()
            .map(|member| member.user_id.clone())
            .collect::<Vec<_>>();
        let added = mentioned
            .into_iter()
            .filter(|member| !mentions(&message).contains(&member.user_id))
            .collect::<Vec<_>>();
        let dropped = mentions(&message)
            .iter()
            .filter(|user| !now.contains(user))
            .cloned()
            .collect::<Vec<_>>();

        message.text = edit.text;
        message.mentions = Some(now);
        message.rev += 1;
        message.edited_at = Some(now_ms());

        let id = parse_id(&message.id)?;
        let mut batch = Batch::new();
        batch.put_value(
            MESSAGES,
            message_key(&guild, &message.channel_id, id),
            &message,
        );
        let channel = message.channel_id.clone();
        self.count_mentions(&mut batch, &guild, &channel, id, &added, 1)?;
        let dropped = self.members_of(&guild, &dropped);
        self.count_mentions(&mut batch, &guild, &channel, id, &dropped, -1)?;
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
        let _guard = self.lock_writable(&guild).await?;
        let member = self.authorize(&guild, &user, Permission::View)?;
        let message = self.message(&guild, &channel, &message_id)?;
        if message.author_id != user && member.role != Role::Owner {
            return Err(MessageCellError::Forbidden);
        }
        let mut record = self.guild(&guild)?;

        let id = parse_id(&message.id)?;
        let mut batch = Batch::new();
        batch.delete(MESSAGES, message_key(&guild, &channel, id));
        let mentioned = self.members_of(&guild, mentions(&message));
        self.count_mentions(&mut batch, &guild, &channel, id, &mentioned, -1)?;
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                deleted: Some(DeletedMessage {
                    channel_id: channel,
                    message_id,
                    attachments: message.attachments.map(|attachments| {
                        attachments
                            .into_iter()
                            .map(|attachment| attachment.id)
                            .collect()
                    }),
                }),
                ..event(GuildEventKind::MessageDeleted)
            },
            true,
        );
        self.commit(batch, &record)
    }

    /// Adds (`add`) or takes back one member's reaction. Neither is an
    /// edit: the text, its revision and its translations stay.
    async fn react(
        &self,
        guild: String,
        channel: String,
        message_id: String,
        user: String,
        emoji: &str,
        add: bool,
    ) -> CellResult<Message> {
        let emoji = self::emoji(emoji)?;
        let _guard = self.lock_writable(&guild).await?;
        let permission = if add {
            Permission::SendMessages
        } else {
            Permission::View
        };
        self.authorize(&guild, &user, permission)?;
        let mut message = self.message(&guild, &channel, &message_id)?;
        let reactions = message.reactions.get_or_insert_with(Vec::new);
        let at = reactions
            .iter()
            .position(|reaction| same_emoji(&reaction.emoji, &emoji));
        // As first used, so everyone's reactions show as one.
        let emoji = at.map_or(emoji, |at| reactions[at].emoji.clone());
        let count = match (add, at) {
            (true, Some(at)) if !reactions[at].user_ids.contains(&user) => {
                reactions[at].user_ids.push(user.clone());
                reactions[at].user_ids.len()
            }
            (true, None) if reactions.len() < MAX_REACTIONS => {
                reactions.push(Reaction {
                    emoji: emoji.clone(),
                    user_ids: vec![user.clone()],
                });
                1
            }
            (true, None) => return Err(MessageCellError::InvalidRequest),
            (false, Some(at)) if reactions[at].user_ids.contains(&user) => {
                reactions[at].user_ids.retain(|known| *known != user);
                let count = reactions[at].user_ids.len();
                if count == 0 {
                    reactions.remove(at);
                }
                count
            }
            // Already so.
            _ => return Ok(message),
        };

        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        batch.put_value(
            MESSAGES,
            message_key(&guild, &channel, parse_id(&message.id)?),
            &message,
        );
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                reaction: Some(ReactionChange {
                    channel_id: message.channel_id.clone(),
                    message_id: message.id.clone(),
                    emoji,
                    user_id: user,
                    count: count as i64,
                }),
                ..event(if add {
                    GuildEventKind::ReactionAdded
                } else {
                    GuildEventKind::ReactionRemoved
                })
            },
            false,
        );
        self.commit(batch, &record)?;
        Ok(message)
    }

    /// Only the log hears of it: Edges pass it to sessions, and it is
    /// trimmed with the rest.
    async fn typing(&self, guild: String, channel: String, user: String) -> CellResult<()> {
        let _guard = self.lock_writable(&guild).await?;
        self.authorize(&guild, &user, Permission::SendMessages)?;
        if self.channel(&guild, &channel)?.kind == ChannelKind::Category {
            return Err(MessageCellError::InvalidRequest);
        }
        let mut record = self.guild(&guild)?;
        let mut batch = Batch::new();
        self.append(
            &mut batch,
            &mut record,
            GuildEvent {
                typing: Some(Typing {
                    channel_id: channel,
                    user_id: user,
                }),
                ..event(GuildEventKind::Typing)
            },
            false,
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
        // A deleted guild's log is read to its end, then the guild is gone
        // (and for followers that missed the end, trimmed meanwhile).
        let record = self.guild_log(&guild)?;
        let head = record.last_seq;
        if record.deleted && (after >= head || after < record.trimmed) {
            return Err(MessageCellError::NotFound);
        }
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
            // It may have moved away meanwhile.
            self.guild_log(&guild)?;
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
            trimmed: record.trimmed,
            events,
        })
    }
}

#[async_trait]
impl babel_api_message_cell::types::MessageCell for MessageCell {
    async fn create_guild(&self, body: NewGuild) -> CellResult<Guild> {
        self.inner.create_guild(body).await
    }

    async fn update_guild(&self, body: GuildUpdate) -> CellResult<Guild> {
        self.inner.update_guild(body).await
    }

    async fn delete_guild(&self, guild_id: String, user_id: String) -> CellResult<()> {
        self.inner.delete_guild(guild_id, user_id).await
    }

    async fn get_guild(&self, guild_id: String, user_id: String) -> CellResult<GuildSnapshot> {
        self.inner
            .authorize(&guild_id, &user_id, Permission::View)?;
        self.inner.snapshot(&self.inner.guild(&guild_id)?, &user_id)
    }

    async fn join_guild(
        &self,
        guild_id: String,
        invite: String,
        user_id: String,
        user_name: String,
    ) -> CellResult<GuildSnapshot> {
        self.inner
            .join_guild(guild_id, invite, user_id, user_name)
            .await
    }

    async fn create_invite(&self, body: NewInvite) -> CellResult<Invite> {
        self.inner.create_invite(body).await
    }

    async fn revoke_invite(
        &self,
        guild_id: String,
        secret: String,
        user_id: String,
    ) -> CellResult<()> {
        self.inner.revoke_invite(guild_id, secret, user_id).await
    }

    async fn get_invites(&self, guild_id: String, user_id: String) -> CellResult<Vec<Invite>> {
        self.inner.get_invites(guild_id, user_id).await
    }

    async fn leave_guild(&self, guild_id: String, user_id: String) -> CellResult<()> {
        self.inner.leave_guild(guild_id, user_id).await
    }

    async fn remove_member(
        &self,
        guild_id: String,
        user_id: String,
        member_id: String,
        ban: bool,
    ) -> CellResult<()> {
        self.inner
            .remove_member(guild_id, user_id, member_id, ban)
            .await
    }

    async fn unban_member(
        &self,
        guild_id: String,
        user_id: String,
        member_id: String,
    ) -> CellResult<()> {
        self.inner.unban_member(guild_id, user_id, member_id).await
    }

    async fn get_bans(&self, guild_id: String, user_id: String) -> CellResult<Vec<Ban>> {
        self.inner.get_bans(guild_id, user_id)
    }

    async fn ack(
        &self,
        guild_id: String,
        channel_id: String,
        user_id: String,
        message_id: String,
    ) -> CellResult<ReadState> {
        self.inner
            .ack(guild_id, channel_id, user_id, message_id)
            .await
    }

    async fn put_notification_settings(
        &self,
        body: NotificationSettings,
        user_id: String,
    ) -> CellResult<NotificationSettings> {
        self.inner.put_notification_settings(user_id, body).await
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

    async fn update_channel(&self, body: ChannelUpdate) -> CellResult<Channel> {
        self.inner.update_channel(body).await
    }

    async fn delete_channel(
        &self,
        guild_id: String,
        channel_id: String,
        user_id: String,
    ) -> CellResult<()> {
        self.inner
            .delete_channel(guild_id, channel_id, user_id)
            .await
    }

    async fn add_upload(&self, body: NewUpload) -> CellResult<()> {
        self.inner.add_upload(body).await
    }

    async fn create_emoji(&self, body: NewEmoji) -> CellResult<Emoji> {
        self.inner.create_emoji(body).await
    }

    async fn delete_emoji(
        &self,
        guild_id: String,
        emoji_id: String,
        user_id: String,
    ) -> CellResult<()> {
        self.inner.delete_emoji(guild_id, emoji_id, user_id).await
    }

    async fn get_emoji(&self, guild_id: String, user_id: String) -> CellResult<Vec<Emoji>> {
        self.inner
            .authorize(&guild_id, &user_id, Permission::View)?;
        self.inner.emoji_of(&guild_id)
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

    async fn add_reaction(
        &self,
        guild_id: String,
        channel_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
    ) -> CellResult<Message> {
        self.inner
            .react(guild_id, channel_id, message_id, user_id, &emoji, true)
            .await
    }

    async fn remove_reaction(
        &self,
        guild_id: String,
        channel_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
    ) -> CellResult<Message> {
        self.inner
            .react(guild_id, channel_id, message_id, user_id, &emoji, false)
            .await
    }

    async fn typing(
        &self,
        guild_id: String,
        channel_id: String,
        user_id: String,
    ) -> CellResult<()> {
        self.inner.typing(guild_id, channel_id, user_id).await
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

    async fn freeze_guild(&self, guild_id: String) -> CellResult<()> {
        self.inner.freeze_guild(&guild_id).await
    }

    async fn export_guild(
        &self,
        guild_id: String,
        cursor: Option<String>,
        limit: i64,
    ) -> CellResult<GuildExport> {
        self.inner.export_guild(&guild_id, cursor, limit)
    }

    async fn import_guild(&self, body: GuildImport) -> CellResult<()> {
        self.inner.import_guild(body)
    }

    async fn finish_move(&self, guild_id: String, moved: bool) -> CellResult<()> {
        self.inner.finish_move(&guild_id, moved).await
    }

    async fn discard_guild(&self, guild_id: String) -> CellResult<()> {
        self.inner.discard_guild(&guild_id).await
    }
}
