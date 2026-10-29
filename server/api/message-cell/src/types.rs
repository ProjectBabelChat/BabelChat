#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewGuild {
    /// Assigned by the Directory.
    pub guild_id: String,
    /// Translation Cell assigned by the Directory; the outbox forwards here.
    pub translation_cell: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub owner_name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum MessageCellError {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "unavailable")]
    Unavailable,
    /// The guild lives on another cell now: ask the Directory again.
    #[serde(rename = "moved")]
    Moved,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildUpdate {
    pub guild_id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildSnapshot {
    pub guild: Guild,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
    /// The requesting member's, one per channel.
    pub read_states: Vec<ReadState>,
    pub notifications: NotificationSettings,
    /// Sequence number of the last event included in this snapshot.
    pub last_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Channel {
    pub id: String,
    pub guild_id: String,
    pub kind: ChannelKind,
    pub name: String,
    pub topic: Option<String>,
    /// Category of a channel, or channel of a thread.
    pub parent_id: Option<String>,
    /// For threads: the message the thread was started from.
    pub starter_message_id: Option<String>,
    pub created_at: i64,
    /// Newest message ever sent here; compared with read states to tell
    /// unread channels.
    pub last_message_id: Option<String>,
    /// Who made it; absent on channels made before this was kept. A
    /// thread's creator may rename and delete it.
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ChannelKind {
    #[serde(rename = "category")]
    Category,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "thread")]
    Thread,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Member {
    pub user_id: String,
    pub name: String,
    pub role: Role,
    pub joined_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Role {
    #[serde(rename = "owner")]
    Owner,
    #[serde(rename = "member")]
    Member,
}

/// How far one member has read one channel.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReadState {
    pub channel_id: String,
    /// Later messages are unread. Starts at the member's join time, so it
    /// may name no message.
    pub last_read_id: String,
    /// Messages after `last_read_id` that mention the member.
    pub mention_count: i64,
}

/// What one member wants to be notified of in one guild. Channel entries
/// override the guild's level; a thread follows its channel unless it has
/// its own entry.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NotificationSettings {
    pub guild_id: String,
    pub level: NotifyLevel,
    pub channels: Vec<ChannelNotify>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum NotifyLevel {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "mentions")]
    Mentions,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ChannelNotify {
    pub channel_id: String,
    pub level: NotifyLevel,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewInvite {
    pub guild_id: String,
    pub user_id: String,
    pub expires_at: Option<i64>,
    pub max_uses: Option<i64>,
}

/// Lets anyone holding it join the guild. Lives with the guild, so it moves
/// with it and dies with it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Invite {
    /// Random; the Edge builds the public code from it and the guild ID.
    pub secret: String,
    pub guild_id: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: Option<i64>,
    pub uses: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Ban {
    pub user_id: String,
    pub name: String,
    pub banned_by: String,
    pub banned_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Permission {
    #[serde(rename = "view")]
    View,
    #[serde(rename = "send_messages")]
    SendMessages,
    #[serde(rename = "create_invite")]
    CreateInvite,
    #[serde(rename = "manage_guild")]
    ManageGuild,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewChannel {
    pub guild_id: String,
    pub user_id: String,
    pub kind: ChannelKind,
    pub name: String,
    pub topic: Option<String>,
    pub parent_id: Option<String>,
    pub starter_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ChannelUpdate {
    pub guild_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub name: String,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewMessage {
    pub guild_id: String,
    pub channel_id: String,
    pub user_id: String,
    /// The author's locale, forwarded to the Translation Cell to complete
    /// language detection.
    pub author_locale: Option<String>,
    pub text: String,
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Message {
    /// Time-ordered and unique within the guild. Carries no cell number.
    pub id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub author_id: String,
    pub author_name: String,
    /// Forwarded to the Translation Cell to complete language detection.
    /// Internal: Edges do not expose it.
    pub author_locale: Option<String>,
    pub text: String,
    /// A message in the same channel this one answers.
    pub reply_to: Option<String>,
    /// Members this message notifies: `@name`s in the text, and the author
    /// of the message replied to. Set by the cell; absent on messages stored
    /// before mentions existed.
    pub mentions: Option<Vec<String>>,
    /// Starts at 1 and increases on every edit.
    pub rev: i64,
    pub created_at: i64,
    pub edited_at: Option<i64>,
    /// In the order first used. Reacting changes neither `rev` nor
    /// `edited_at`. Absent on messages stored before reactions existed.
    pub reactions: Option<Vec<Reaction>>,
}

/// Everyone who reacted to a message with one emoji.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Reaction {
    /// A Unicode emoji. Room is left for a guild's own emoji as
    /// `custom:<id>`, usable in other guilds too; none exist yet.
    pub emoji: String,
    /// In the order they reacted.
    pub user_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageEdit {
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub user_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildEventBatch {
    pub events: Vec<GuildEvent>,
    /// Pass as `after` to the next poll.
    pub next: i64,
    /// Sequence number of the newest event of the guild. Polling with an
    /// `after` beyond it returns at once, so followers can start at the head.
    pub head: i64,
    /// Events up to this sequence number were trimmed from the log. A
    /// follower whose `after` is below it has missed some.
    pub trimmed: i64,
}

/// One entry of a guild's event log. Exactly one payload is set, chosen by
/// `kind`; events about one member also name them in `user_id`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildEvent {
    pub seq: i64,
    pub kind: GuildEventKind,
    pub guild: Option<Guild>,
    pub member: Option<Member>,
    pub channel: Option<Channel>,
    pub message: Option<Message>,
    pub deleted: Option<DeletedMessage>,
    pub reaction: Option<ReactionChange>,
    pub typing: Option<Typing>,
    pub user_id: Option<String>,
    pub read_state: Option<ReadState>,
    pub notifications: Option<NotificationSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum GuildEventKind {
    #[serde(rename = "guild_created")]
    GuildCreated,
    /// Name or description (`guild`).
    #[serde(rename = "guild_updated")]
    GuildUpdated,
    /// The guild's last event but its members' removals, which follow
    /// (`member_removed`, one per member).
    #[serde(rename = "guild_deleted")]
    GuildDeleted,
    #[serde(rename = "member_joined")]
    MemberJoined,
    #[serde(rename = "member_left")]
    MemberLeft,
    /// Kicked or banned.
    #[serde(rename = "member_removed")]
    MemberRemoved,
    #[serde(rename = "channel_created")]
    ChannelCreated,
    /// Renamed, a new topic, or out of a deleted category.
    #[serde(rename = "channel_updated")]
    ChannelUpdated,
    /// With its messages. A channel's threads go first, each with its own
    /// event.
    #[serde(rename = "channel_deleted")]
    ChannelDeleted,
    #[serde(rename = "message_created")]
    MessageCreated,
    #[serde(rename = "message_edited")]
    MessageEdited,
    #[serde(rename = "message_deleted")]
    MessageDeleted,
    #[serde(rename = "reaction_added")]
    ReactionAdded,
    #[serde(rename = "reaction_removed")]
    ReactionRemoved,
    /// Not kept anywhere but the log; Edges pass it on and forget it.
    #[serde(rename = "typing")]
    Typing,
    /// One member's read marker moved (`user_id`, `read_state`). Edges pass
    /// it to that member's sessions only, on whichever Edge they are.
    #[serde(rename = "read_state_updated")]
    ReadStateUpdated,
    /// One member's notification settings changed (`user_id`,
    /// `notifications`); for that member's sessions only.
    #[serde(rename = "notifications_updated")]
    NotificationsUpdated,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeletedMessage {
    pub channel_id: String,
    pub message_id: String,
}

/// One member added or took back one reaction.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReactionChange {
    pub channel_id: String,
    pub message_id: String,
    pub emoji: String,
    pub user_id: String,
    /// How many reacted with `emoji` after the change, so applying the
    /// same change twice does no harm.
    pub count: i64,
}

/// One member is writing in a channel.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Typing {
    pub channel_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildExport {
    pub entries: Vec<GuildEntry>,
    /// Pass to the next export; null once everything is exported.
    pub cursor: Option<String>,
}

/// One stored entry of a guild, on its way to another cell. Keys and values
/// are hex: they are the cell's own storage format.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildEntry {
    pub table: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildImport {
    pub guild_id: String,
    pub entries: Vec<GuildEntry>,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait MessageCell: Send + Sync + 'static {
    async fn create_guild(&self, body: NewGuild) -> Result<Guild, MessageCellError>;
    /// Managers only.
    async fn update_guild(&self, body: GuildUpdate) -> Result<Guild, MessageCellError>;
    /// The owner only. Everything the guild holds goes; only its log stays
    /// a while, for Edges to read to the end, and its record, answering
    /// `not_found`.
    async fn delete_guild(&self, guild_id: String, user_id: String) -> Result<(), MessageCellError>;
    /// Fails with `forbidden` unless the user is a member.
    async fn get_guild(&self, guild_id: String, user_id: String) -> Result<GuildSnapshot, MessageCellError>;
    /// Fails with `not_found` unless the invite is valid. Members joining
    /// again succeed whatever the invite.
    async fn join_guild(&self, guild_id: String, invite: String, user_id: String, user_name: String) -> Result<GuildSnapshot, MessageCellError>;
    async fn create_invite(&self, body: NewInvite) -> Result<Invite, MessageCellError>;
    /// The invite's creator or a manager may revoke it.
    async fn revoke_invite(&self, guild_id: String, secret: String, user_id: String) -> Result<(), MessageCellError>;
    /// Valid invites: all of them for managers, a member's own otherwise.
    async fn get_invites(&self, guild_id: String, user_id: String) -> Result<Vec<Invite>, MessageCellError>;
    /// The owner cannot leave (`invalid_request`).
    async fn leave_guild(&self, guild_id: String, user_id: String) -> Result<(), MessageCellError>;
    /// Managers only. With `ban`, invites no longer let the member back in.
    async fn remove_member(&self, guild_id: String, user_id: String, member_id: String, ban: bool) -> Result<(), MessageCellError>;
    async fn unban_member(&self, guild_id: String, user_id: String, member_id: String) -> Result<(), MessageCellError>;
    async fn get_bans(&self, guild_id: String, user_id: String) -> Result<Vec<Ban>, MessageCellError>;
    /// Marks the channel read up to `message_id` (never backwards) and
    /// recounts the mentions after it.
    async fn ack(&self, guild_id: String, channel_id: String, user_id: String, message_id: String) -> Result<ReadState, MessageCellError>;
    async fn put_notification_settings(&self, body: NotificationSettings, user_id: String) -> Result<NotificationSettings, MessageCellError>;
    async fn check_permission(&self, guild_id: String, user_id: String, permission: Permission) -> Result<(), MessageCellError>;
    async fn create_channel(&self, body: NewChannel) -> Result<Channel, MessageCellError>;
    /// Managers; for a thread, its creator too.
    async fn update_channel(&self, body: ChannelUpdate) -> Result<Channel, MessageCellError>;
    /// Managers; for a thread, its creator too. A channel's threads and
    /// messages go with it; a category's channels stay, outside it.
    async fn delete_channel(&self, guild_id: String, channel_id: String, user_id: String) -> Result<(), MessageCellError>;
    async fn send_message(&self, body: NewMessage) -> Result<Message, MessageCellError>;
    async fn edit_message(&self, body: MessageEdit) -> Result<Message, MessageCellError>;
    async fn delete_message(&self, guild_id: String, channel_id: String, message_id: String, user_id: String) -> Result<(), MessageCellError>;
    /// Adding a reaction already there, or removing one that is not, changes
    /// nothing. A message holds at most 20 different emoji.
    async fn add_reaction(&self, guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Result<Message, MessageCellError>;
    async fn remove_reaction(&self, guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Result<Message, MessageCellError>;
    /// Tells the guild's readers the member is writing in the channel.
    async fn typing(&self, guild_id: String, channel_id: String, user_id: String) -> Result<(), MessageCellError>;
    /// Newest first, strictly before `before` when given.
    async fn get_messages(&self, guild_id: String, channel_id: String, user_id: String, before: Option<String>, limit: i64) -> Result<Vec<Message>, MessageCellError>;
    /// Long poll on the guild's event log, for Edges.
    async fn poll_events(&self, guild_id: String, after: i64, wait_ms: i64) -> Result<GuildEventBatch, MessageCellError>;
    /// Holds the guild's writes (they wait) and returns once its outbox has
    /// delivered everything, so the guild can be copied as it is.
    async fn freeze_guild(&self, guild_id: String) -> Result<(), MessageCellError>;
    /// A page of the frozen guild's entries, from `cursor` on.
    async fn export_guild(&self, guild_id: String, cursor: Option<String>, limit: i64) -> Result<GuildExport, MessageCellError>;
    /// Stores exported entries. The guild becomes whole here with its last
    /// page, but is only reached once the Directory points at it.
    async fn import_guild(&self, body: GuildImport) -> Result<(), MessageCellError>;
    /// Ends a move at the old cell: `moved` drops the guild here (calls
    /// about it answer `moved` from now on); otherwise writes resume.
    async fn finish_move(&self, guild_id: String, moved: bool) -> Result<(), MessageCellError>;
    /// Drops whatever this cell holds of a guild: an import that did not
    /// finish.
    async fn discard_guild(&self, guild_id: String) -> Result<(), MessageCellError>;
}

