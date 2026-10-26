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
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildSnapshot {
    pub guild: Guild,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Permission {
    #[serde(rename = "view")]
    View,
    #[serde(rename = "send_messages")]
    SendMessages,
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
pub struct NewMessage {
    pub guild_id: String,
    pub channel_id: String,
    pub user_id: String,
    /// The author's locale, forwarded to the Translation Cell to complete
    /// language detection.
    pub author_locale: Option<String>,
    pub text: String,
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
    /// Starts at 1 and increases on every edit.
    pub rev: i64,
    pub created_at: i64,
    pub edited_at: Option<i64>,
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
}

/// One entry of a guild's event log. Exactly one payload is set, chosen by
/// `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildEvent {
    pub seq: i64,
    pub kind: GuildEventKind,
    pub guild: Option<Guild>,
    pub member: Option<Member>,
    pub channel: Option<Channel>,
    pub message: Option<Message>,
    pub deleted: Option<DeletedMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum GuildEventKind {
    #[serde(rename = "guild_created")]
    GuildCreated,
    #[serde(rename = "member_joined")]
    MemberJoined,
    #[serde(rename = "channel_created")]
    ChannelCreated,
    #[serde(rename = "message_created")]
    MessageCreated,
    #[serde(rename = "message_edited")]
    MessageEdited,
    #[serde(rename = "message_deleted")]
    MessageDeleted,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeletedMessage {
    pub channel_id: String,
    pub message_id: String,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait MessageCell: Send + Sync + 'static {
    async fn create_guild(&self, body: NewGuild) -> Result<Guild, MessageCellError>;
    /// Fails with `forbidden` unless the user is a member.
    async fn get_guild(&self, guild_id: String, user_id: String) -> Result<GuildSnapshot, MessageCellError>;
    async fn join_guild(&self, guild_id: String, user_id: String, user_name: String) -> Result<GuildSnapshot, MessageCellError>;
    async fn check_permission(&self, guild_id: String, user_id: String, permission: Permission) -> Result<(), MessageCellError>;
    async fn create_channel(&self, body: NewChannel) -> Result<Channel, MessageCellError>;
    async fn send_message(&self, body: NewMessage) -> Result<Message, MessageCellError>;
    async fn edit_message(&self, body: MessageEdit) -> Result<Message, MessageCellError>;
    async fn delete_message(&self, guild_id: String, channel_id: String, message_id: String, user_id: String) -> Result<(), MessageCellError>;
    /// Newest first, strictly before `before` when given.
    async fn get_messages(&self, guild_id: String, channel_id: String, user_id: String, before: Option<String>, limit: i64) -> Result<Vec<Message>, MessageCellError>;
    /// Long poll on the guild's event log, for Edges.
    async fn poll_events(&self, guild_id: String, after: i64, wait_ms: i64) -> Result<GuildEventBatch, MessageCellError>;
}

