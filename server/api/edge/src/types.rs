#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Registration {
    pub name: String,
    pub password: String,
    /// BCP 47, e.g. "ja-JP".
    pub locale: String,
    /// Needed when the server takes new accounts by invitation only.
    pub signup_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Token {
    pub jwt: String,
    pub user_id: String,
    /// As registered; sign-in ignores case.
    pub name: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ApiError {
    /// Also returned when the guild exists but the user may not see it, so
    /// IDs cannot be probed.
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "invalid_language")]
    InvalidLanguage,
    #[serde(rename = "limit_exceeded")]
    LimitExceeded,
    #[serde(rename = "name_taken")]
    NameTaken,
    #[serde(rename = "invalid_credentials")]
    InvalidCredentials,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "unavailable")]
    Unavailable,
    /// The server takes no new accounts.
    #[serde(rename = "signups_closed")]
    SignupsClosed,
    /// The server takes new accounts by invitation, and the code is
    /// missing, expired or used up.
    #[serde(rename = "invalid_signup_code")]
    InvalidSignupCode,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Credentials {
    pub name: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Claim {
    /// User ID.
    pub sub: String,
    pub name: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct UserSettings {
    pub locale: String,
    /// Translate on device even languages a guild does not declare.
    pub translate_outside_set: bool,
    pub show_original: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PasswordChange {
    pub password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewGuild {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildView {
    pub guild: Guild,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
    pub policy: LanguagePolicy,
    pub language: LanguageView,
    /// The user's, one per channel.
    pub read_states: Vec<ReadState>,
    pub notifications: NotificationSettings,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Channel {
    pub id: String,
    pub guild_id: String,
    pub kind: ChannelKind,
    pub name: String,
    pub topic: Option<String>,
    pub parent_id: Option<String>,
    pub starter_message_id: Option<String>,
    pub created_at: i64,
    /// Newest message ever sent here. The channel is unread while it is
    /// newer than the read state's `last_read_id`.
    pub last_message_id: Option<String>,
    /// Who made it, when known. A thread's creator may rename and
    /// delete it.
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
    pub owner: bool,
    pub joined_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LanguagePolicy {
    pub guild_id: String,
    /// "free", "paid1", "paid2" or "paid3".
    pub plan: String,
    pub languages: Vec<String>,
    pub llm_languages: Vec<String>,
    pub max_languages: i64,
    pub max_llm_languages: i64,
    pub region_mode: RegionMode,
    pub aliases: Vec<LanguageAlias>,
    pub translate_regional_variants: bool,
    /// What the LLM translates with, one of `get_models`. Null: the
    /// default model.
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum RegionMode {
    #[serde(rename = "merged")]
    Merged,
    #[serde(rename = "distinct")]
    Distinct,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LanguageAlias {
    pub from: String,
    pub to: String,
}

/// How the current user reads a guild. Computed by the Edge from the user's
/// settings and the guild's policy, so clients need no normalization tables.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LanguageView {
    /// The user's locale normalized with the guild's policy.
    pub effective: String,
    pub in_set: bool,
    /// Translations into `effective` come from the LLM.
    pub llm: bool,
    /// The client should translate on device into `effective`.
    pub client_translation: bool,
}

/// How far the user has read one channel.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReadState {
    pub channel_id: String,
    /// Later messages are unread. Starts at the member's join time, so it
    /// may name no message.
    pub last_read_id: String,
    /// Messages after `last_read_id` that mention the user.
    pub mention_count: i64,
}

/// What the user wants to be notified of in one guild. Channel entries
/// override the guild's level; a thread follows its channel unless it has
/// its own entry. Clients decide what to show; the server keeps the choice.
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
    /// Seconds until the invite expires: 7 days when absent, never when 0.
    pub max_age_secs: Option<i64>,
    pub max_uses: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Invite {
    /// Share this: anyone holding it can join until it expires or runs out.
    pub code: String,
    pub guild_id: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: Option<i64>,
    pub uses: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GuildUpdate {
    pub guild_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewChannel {
    pub guild_id: String,
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
    pub name: String,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LanguagePolicyUpdate {
    pub guild_id: String,
    pub languages: Vec<String>,
    pub llm_languages: Vec<String>,
    pub region_mode: RegionMode,
    pub aliases: Vec<LanguageAlias>,
    pub translate_regional_variants: bool,
    pub model: Option<String>,
}

/// A model a guild can translate with.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslationModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Ban {
    pub user_id: String,
    pub name: String,
    pub banned_by: String,
    pub banned_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewMessage {
    pub guild_id: String,
    pub channel_id: String,
    /// `@name` notifies that member.
    pub text: String,
    pub reply_to: Option<String>,
    /// IDs of the user's uploads to this channel (`POST /media/upload`),
    /// at most 10; with any, `text` may be empty.
    pub attachments: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageView {
    pub message: Message,
    pub translation: Option<MessageTranslation>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Message {
    pub id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub author_id: String,
    pub author_name: String,
    pub text: String,
    /// A message in the same channel this one answers.
    pub reply_to: Option<String>,
    /// Users this message notifies: members named with `@name` in the text,
    /// and the author of the message replied to. Set by the server.
    pub mentions: Vec<String>,
    pub rev: i64,
    pub created_at: i64,
    pub edited_at: Option<i64>,
    /// In the order first used. Reacting changes neither `rev` nor
    /// `edited_at`.
    pub reactions: Vec<Reaction>,
    /// In the order sent. Edits keep them.
    pub attachments: Vec<Attachment>,
}

/// One emoji people reacted with.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Reaction {
    /// A Unicode emoji. Later also `custom:<id>`, a guild's own emoji
    /// (usable in other guilds too); none exist yet.
    pub emoji: String,
    pub count: i64,
    /// The user is among them.
    pub me: bool,
}

/// A file sent with a message.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    /// Read from the file by the server, not taken from the sender:
    /// `image/png`, `image/jpeg`, `image/gif`, `image/webp`, a few
    /// others, else `application/octet-stream`.
    pub content_type: String,
    pub size: i64,
    /// Of images.
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Where to download it on this server: `GET` with the token, like the
    /// API. Images come back as themselves, anything else as a download
    /// (`Content-Disposition: attachment`).
    pub url: String,
}

/// Translations of one message, filtered to what the user needs: their own
/// language and English (the pivot for on-device translation).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageTranslation {
    pub message_id: String,
    pub channel_id: String,
    /// Revision of the text these translations were made from. Ignore them
    /// when it differs from the message's `rev`.
    pub rev: i64,
    /// Detected source language; on-device translation starts from it.
    pub source_lang: String,
    /// "done", "pending", "budget_exceeded" or "failed".
    pub status: String,
    pub translations: Vec<Translation>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Translation {
    pub lang: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageEdit {
    pub guild_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Ready {
    pub session_id: String,
    pub user_id: String,
    pub guilds: Vec<GuildView>,
    /// Pass as `after` to the first poll.
    pub cursor: i64,
}

/// When `poll` fails with `not_found` the session is gone (the Edge
/// restarted, or the client fell further behind than the Edge keeps):
/// connect again and reload what is on screen.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct EventBatch {
    pub events: Vec<Event>,
    /// Pass as `after` to the next poll.
    pub next: i64,
}

/// One event for this session. Exactly one payload is set, chosen by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Event {
    pub seq: i64,
    pub kind: EventKind,
    pub guild_id: String,
    pub guild: Option<GuildView>,
    /// For guild_updated.
    pub guild_info: Option<Guild>,
    pub member: Option<Member>,
    pub channel: Option<Channel>,
    pub message: Option<Message>,
    pub deleted: Option<DeletedMessage>,
    pub reaction: Option<ReactionChange>,
    pub typing: Option<Typing>,
    pub translation: Option<MessageTranslation>,
    /// For policy_updated: the new policy and the user's resulting view.
    pub policy: Option<LanguagePolicy>,
    pub language: Option<LanguageView>,
    pub read_state: Option<ReadState>,
    pub notifications: Option<NotificationSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum EventKind {
    #[serde(rename = "guild_joined")]
    GuildJoined,
    /// Renamed, or a new description (`guild_info`).
    #[serde(rename = "guild_updated")]
    GuildUpdated,
    /// The owner deleted it: forget it.
    #[serde(rename = "guild_deleted")]
    GuildDeleted,
    #[serde(rename = "member_joined")]
    MemberJoined,
    /// For the leaving user too: their last event from the guild.
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
    #[serde(rename = "typing")]
    Typing,
    #[serde(rename = "message_translated")]
    MessageTranslated,
    #[serde(rename = "policy_updated")]
    PolicyUpdated,
    /// The user read a channel, here or on another device.
    #[serde(rename = "read_state_updated")]
    ReadStateUpdated,
    #[serde(rename = "notifications_updated")]
    NotificationsUpdated,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeletedMessage {
    pub channel_id: String,
    pub message_id: String,
}

/// Someone added or took back a reaction.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReactionChange {
    pub channel_id: String,
    pub message_id: String,
    pub emoji: String,
    pub user_id: String,
    /// How many reacted with `emoji` after the change: set it, do not add.
    pub count: i64,
}

/// Someone is writing. Show it for about 10 seconds, or until their
/// message arrives.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Typing {
    pub channel_id: String,
    pub user_id: String,
}



pub(crate) fn __bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    let auth = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let prefix = "Bearer ";
    auth.strip_prefix(prefix).or_else(|| auth.strip_prefix(prefix.to_lowercase().as_str()))
}


/// Implement this for your server struct.
///
/// ## Example
/// ```
/// impl JwtValidator for YourServerStruct {
///     fn validate<T: serde::de::DeserializeOwned>(&self, token: &str) -> Result<T, ()> {
///         let claim = jsonwebtoken::decode(
///             token,
///             &self.decoding_key,
///             &Validation::default(),
///         )
///         .map_err(|_| ())?
///         .claims;
///
///         Ok(claim)
///     }
/// }
/// ```
pub trait JwtValidator {
    fn validate<T: serde::de::DeserializeOwned>(&self, token: &str) -> Result<T, ()>;
}


use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait Auth: Send + Sync + 'static {
    async fn register(&self, body: Registration) -> Result<Token, ApiError>;
    async fn login(&self, body: Credentials) -> Result<Token, ApiError>;
}

#[async_trait]
pub trait Me: Send + Sync + 'static {
    async fn get_settings(&self, claim: Claim) -> Result<UserSettings, ApiError>;
    async fn put_settings(&self, claim: Claim, body: UserSettings) -> Result<UserSettings, ApiError>;
    /// Signs the user's other devices out, like `sign_out_others`. A wrong
    /// current password is `invalid_credentials`.
    async fn change_password(&self, claim: Claim, body: PasswordChange) -> Result<Token, ApiError>;
    /// Every other token of the user stops working and the user's sessions
    /// end; the answer is a new token for this device (its own session ends
    /// too: connect again with the new token).
    async fn sign_out_others(&self, claim: Claim) -> Result<Token, ApiError>;
}

#[async_trait]
pub trait Guilds: Send + Sync + 'static {
    async fn create_guild(&self, claim: Claim, body: NewGuild) -> Result<GuildView, ApiError>;
    /// Any member may invite.
    async fn create_invite(&self, claim: Claim, body: NewInvite) -> Result<Invite, ApiError>;
    /// The invite's creator or the owner may revoke it.
    async fn revoke_invite(&self, claim: Claim, code: String) -> Result<(), ApiError>;
    /// Valid invites: all of them for the owner, the user's own otherwise.
    async fn get_invites(&self, claim: Claim, guild_id: String) -> Result<Vec<Invite>, ApiError>;
    /// Joins with an invite code. Invalid, expired and used-up codes are all
    /// `not_found`.
    async fn join_guild(&self, claim: Claim, invite: String) -> Result<GuildView, ApiError>;
    async fn get_guild(&self, claim: Claim, guild_id: String) -> Result<GuildView, ApiError>;
    /// Owner only.
    async fn update_guild(&self, claim: Claim, body: GuildUpdate) -> Result<Guild, ApiError>;
    /// Owner only. Deletes everything the guild holds, for good.
    async fn delete_guild(&self, claim: Claim, guild_id: String) -> Result<(), ApiError>;
    async fn create_channel(&self, claim: Claim, body: NewChannel) -> Result<Channel, ApiError>;
    /// The owner; for a thread, its creator too.
    async fn update_channel(&self, claim: Claim, body: ChannelUpdate) -> Result<Channel, ApiError>;
    /// The owner; for a thread, its creator too. A channel's threads and
    /// messages go with it; a category's channels stay, outside it.
    async fn delete_channel(&self, claim: Claim, guild_id: String, channel_id: String) -> Result<(), ApiError>;
    /// Only the owner may change the policy. Languages are normalized and
    /// counted after normalization.
    async fn put_language_policy(&self, claim: Claim, body: LanguagePolicyUpdate) -> Result<LanguagePolicy, ApiError>;
    /// Models the guild may translate with; the first is the default.
    async fn get_models(&self, claim: Claim, guild_id: String) -> Result<Vec<TranslationModel>, ApiError>;
    /// The owner cannot leave (`invalid_request`).
    async fn leave_guild(&self, claim: Claim, guild_id: String) -> Result<(), ApiError>;
    /// Owner only. With `ban`, invites no longer let the user back in.
    async fn remove_member(&self, claim: Claim, guild_id: String, user_id: String, ban: bool) -> Result<(), ApiError>;
    async fn unban_member(&self, claim: Claim, guild_id: String, user_id: String) -> Result<(), ApiError>;
    async fn get_bans(&self, claim: Claim, guild_id: String) -> Result<Vec<Ban>, ApiError>;
    /// Saved for the user's other devices; they hear `notifications_updated`.
    async fn put_notification_settings(&self, claim: Claim, body: NotificationSettings) -> Result<NotificationSettings, ApiError>;
}

/// Files go up and down outside the JSON API, with the same token
/// (`Authorization: Bearer`):
///
///   POST /media/upload?guild_id=&channel_id=&name=   body: the file
///       -> Attachment (JSON). Up to 10 MB (`limit_exceeded`); the user
///       must be allowed to write in the channel. Send it within a day.
///   GET  <Attachment.url>  -> the file, for the guild's members.
///
/// Errors are ApiError as JSON with 400, like the API's (401 without a
/// valid token).
#[async_trait]
pub trait Messages: Send + Sync + 'static {
    async fn send_message(&self, claim: Claim, body: NewMessage) -> Result<MessageView, ApiError>;
    async fn edit_message(&self, claim: Claim, body: MessageEdit) -> Result<MessageView, ApiError>;
    async fn delete_message(&self, claim: Claim, guild_id: String, channel_id: String, message_id: String) -> Result<(), ApiError>;
    /// `emoji` is one Unicode emoji. Reacting twice with the same one
    /// changes nothing. A message holds at most 20 different emoji.
    async fn add_reaction(&self, claim: Claim, guild_id: String, channel_id: String, message_id: String, emoji: String) -> Result<Message, ApiError>;
    async fn remove_reaction(&self, claim: Claim, guild_id: String, channel_id: String, message_id: String, emoji: String) -> Result<Message, ApiError>;
    /// Tells the channel's readers the user is writing. Send it at most every
    /// 8 seconds while they do; sending the message ends it.
    async fn typing(&self, claim: Claim, guild_id: String, channel_id: String) -> Result<(), ApiError>;
    /// Newest first, with translations for the user's language and English.
    async fn get_messages(&self, claim: Claim, guild_id: String, channel_id: String, before: Option<String>, limit: i64) -> Result<Vec<MessageView>, ApiError>;
    /// Marks the channel read up to `message_id`; never moves backwards.
    /// Sending a message marks its channel read for the author.
    async fn ack(&self, claim: Claim, guild_id: String, channel_id: String, message_id: String) -> Result<ReadState, ApiError>;
}

/// Events reach a session by long polling (`poll`) or over a WebSocket at
/// `GET /gateway/ws?session_id=...&after=...`, authorized with the same
/// bearer token. The server sends each `EventBatch` as one JSON text frame,
/// as soon as events exist; the client sends nothing. Resume after a drop by
/// opening the socket again with the last `next`. Close code 4404 means the
/// session is gone, like `poll`'s `not_found`.
#[async_trait]
pub trait Gateway: Send + Sync + 'static {
    async fn connect(&self, claim: Claim) -> Result<Ready, ApiError>;
    async fn poll(&self, claim: Claim, session_id: String, after: i64, wait_ms: i64) -> Result<EventBatch, ApiError>;
}

