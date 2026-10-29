use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ThrowsOrOtherError<T> {
    Throws(T),
    Other(reqwest::Error),
}

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

/// A file a member put on a Media service, to send with their next message
/// in the channel. Kept for a day.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewUpload {
    pub guild_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub attachment: Attachment,
}

/// A file sent with a message. Its contents are on a Media service.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Attachment {
    /// "<media service>.<random>": where the file is comes first.
    pub id: String,
    pub name: String,
    /// Read from the file by the Media service.
    pub content_type: String,
    pub size: i64,
    /// Of images.
    pub width: Option<i64>,
    pub height: Option<i64>,
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
    /// Uploads of this member in this channel (`add_upload`), at most 10;
    /// with any, `text` may be empty.
    pub attachments: Option<Vec<String>>,
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
    /// Files sent with it, in the order given. Edits keep them.
    pub attachments: Option<Vec<Attachment>>,
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
    /// Its attachments' IDs, for the outbox to delete them.
    pub attachments: Option<Vec<String>>,
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



pub struct MessageCell {
    pub url: String,
    pub client: reqwest::Client,
}

impl MessageCell {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn create_guild(&self, __body: NewGuild) -> Result<Guild, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/create_guild", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Managers only.
    pub async fn update_guild(&self, __body: GuildUpdate) -> Result<Guild, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/update_guild", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// The owner only. Everything the guild holds goes; only its log stays
    /// a while, for Edges to read to the end, and its record, answering
    /// `not_found`.
    pub async fn delete_guild(&self, guild_id: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/delete_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Fails with `forbidden` unless the user is a member.
    pub async fn get_guild(&self, guild_id: String, user_id: String) -> Result<GuildSnapshot, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/get_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Fails with `not_found` unless the invite is valid. Members joining
    /// again succeed whatever the invite.
    pub async fn join_guild(&self, guild_id: String, invite: String, user_id: String, user_name: String) -> Result<GuildSnapshot, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/join_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("invite", serde_json::to_string(&invite).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("user_name", serde_json::to_string(&user_name).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn create_invite(&self, __body: NewInvite) -> Result<Invite, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/create_invite", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// The invite's creator or a manager may revoke it.
    pub async fn revoke_invite(&self, guild_id: String, secret: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/revoke_invite", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("secret", serde_json::to_string(&secret).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Valid invites: all of them for managers, a member's own otherwise.
    pub async fn get_invites(&self, guild_id: String, user_id: String) -> Result<Vec<Invite>, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/get_invites", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// The owner cannot leave (`invalid_request`).
    pub async fn leave_guild(&self, guild_id: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/leave_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Managers only. With `ban`, invites no longer let the member back in.
    pub async fn remove_member(&self, guild_id: String, user_id: String, member_id: String, ban: bool) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/remove_member", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("member_id", serde_json::to_string(&member_id).unwrap()), ("ban", serde_json::to_string(&ban).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn unban_member(&self, guild_id: String, user_id: String, member_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/unban_member", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("member_id", serde_json::to_string(&member_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn get_bans(&self, guild_id: String, user_id: String) -> Result<Vec<Ban>, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/get_bans", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Marks the channel read up to `message_id` (never backwards) and
    /// recounts the mentions after it.
    pub async fn ack(&self, guild_id: String, channel_id: String, user_id: String, message_id: String) -> Result<ReadState, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/ack", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("message_id", serde_json::to_string(&message_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn put_notification_settings(&self, __body: NotificationSettings, user_id: String) -> Result<NotificationSettings, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/put_notification_settings", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn check_permission(&self, guild_id: String, user_id: String, permission: Permission) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/check_permission", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("permission", serde_json::to_string(&permission).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn create_channel(&self, __body: NewChannel) -> Result<Channel, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/create_channel", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Managers; for a thread, its creator too.
    pub async fn update_channel(&self, __body: ChannelUpdate) -> Result<Channel, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/update_channel", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Managers; for a thread, its creator too. A channel's threads and
    /// messages go with it; a category's channels stay, outside it.
    pub async fn delete_channel(&self, guild_id: String, channel_id: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/delete_channel", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Records a file the Edge put on a Media service for the member, who
    /// must be allowed to write in the channel. Sending a message takes it.
    pub async fn add_upload(&self, __body: NewUpload) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/add_upload", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn send_message(&self, __body: NewMessage) -> Result<Message, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/send_message", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn edit_message(&self, __body: MessageEdit) -> Result<Message, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/edit_message", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn delete_message(&self, guild_id: String, channel_id: String, message_id: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/delete_message", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("message_id", serde_json::to_string(&message_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Adding a reaction already there, or removing one that is not, changes
    /// nothing. A message holds at most 20 different emoji.
    pub async fn add_reaction(&self, guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Result<Message, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/add_reaction", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("message_id", serde_json::to_string(&message_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("emoji", serde_json::to_string(&emoji).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn remove_reaction(&self, guild_id: String, channel_id: String, message_id: String, user_id: String, emoji: String) -> Result<Message, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/remove_reaction", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("message_id", serde_json::to_string(&message_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("emoji", serde_json::to_string(&emoji).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Tells the guild's readers the member is writing in the channel.
    pub async fn typing(&self, guild_id: String, channel_id: String, user_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/typing", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Newest first, strictly before `before` when given.
    pub async fn get_messages(&self, guild_id: String, channel_id: String, user_id: String, before: Option<String>, limit: i64) -> Result<Vec<Message>, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/get_messages", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("user_id", serde_json::to_string(&user_id).unwrap()), ("before", serde_json::to_string(&before).unwrap()), ("limit", serde_json::to_string(&limit).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Long poll on the guild's event log, for Edges.
    pub async fn poll_events(&self, guild_id: String, after: i64, wait_ms: i64) -> Result<GuildEventBatch, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/poll_events", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Holds the guild's writes (they wait) and returns once its outbox has
    /// delivered everything, so the guild can be copied as it is.
    pub async fn freeze_guild(&self, guild_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/freeze_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// A page of the frozen guild's entries, from `cursor` on.
    pub async fn export_guild(&self, guild_id: String, cursor: Option<String>, limit: i64) -> Result<GuildExport, ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.get(format!("{}/message_cell/export_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("cursor", serde_json::to_string(&cursor).unwrap()), ("limit", serde_json::to_string(&limit).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Stores exported entries. The guild becomes whole here with its last
    /// page, but is only reached once the Directory points at it.
    pub async fn import_guild(&self, __body: GuildImport) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/import_guild", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Ends a move at the old cell: `moved` drops the guild here (calls
    /// about it answer `moved` from now on); otherwise writes resume.
    pub async fn finish_move(&self, guild_id: String, moved: bool) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/finish_move", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("moved", serde_json::to_string(&moved).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Drops whatever this cell holds of a guild: an import that did not
    /// finish.
    pub async fn discard_guild(&self, guild_id: String) -> Result<(), ThrowsOrOtherError<MessageCellError>> {
        let mut request = self.client.post(format!("{}/message_cell/discard_guild", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
}
