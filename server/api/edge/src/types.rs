#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Registration {
    pub name: String,
    pub password: String,
    /// BCP 47, e.g. "ja-JP".
    pub locale: String,
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
    pub expires_at: Option<i64>,
    pub max_uses: Option<i64>,
    pub uses: i64,
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
pub struct LanguagePolicyUpdate {
    pub guild_id: String,
    pub languages: Vec<String>,
    pub llm_languages: Vec<String>,
    pub region_mode: RegionMode,
    pub aliases: Vec<LanguageAlias>,
    pub translate_regional_variants: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewMessage {
    pub guild_id: String,
    pub channel_id: String,
    pub text: String,
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
    pub rev: i64,
    pub created_at: i64,
    pub edited_at: Option<i64>,
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
    pub member: Option<Member>,
    pub channel: Option<Channel>,
    pub message: Option<Message>,
    pub deleted: Option<DeletedMessage>,
    pub translation: Option<MessageTranslation>,
    /// For policy_updated: the new policy and the user's resulting view.
    pub policy: Option<LanguagePolicy>,
    pub language: Option<LanguageView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum EventKind {
    #[serde(rename = "guild_joined")]
    GuildJoined,
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
    #[serde(rename = "message_translated")]
    MessageTranslated,
    #[serde(rename = "policy_updated")]
    PolicyUpdated,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DeletedMessage {
    pub channel_id: String,
    pub message_id: String,
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
}

#[async_trait]
pub trait Guilds: Send + Sync + 'static {
    async fn create_guild(&self, claim: Claim, body: NewGuild) -> Result<GuildView, ApiError>;
    /// Any member may invite.
    async fn create_invite(&self, claim: Claim, body: NewInvite) -> Result<Invite, ApiError>;
    /// The invite's creator or the owner may revoke it.
    async fn revoke_invite(&self, claim: Claim, code: String) -> Result<(), ApiError>;
    /// Joins with an invite code. Invalid, expired and used-up codes are all
    /// `not_found`.
    async fn join_guild(&self, claim: Claim, invite: String) -> Result<GuildView, ApiError>;
    async fn get_guild(&self, claim: Claim, guild_id: String) -> Result<GuildView, ApiError>;
    async fn create_channel(&self, claim: Claim, body: NewChannel) -> Result<Channel, ApiError>;
    /// Only the owner may change the policy. Languages are normalized and
    /// counted after normalization.
    async fn put_language_policy(&self, claim: Claim, body: LanguagePolicyUpdate) -> Result<LanguagePolicy, ApiError>;
}

#[async_trait]
pub trait Messages: Send + Sync + 'static {
    async fn send_message(&self, claim: Claim, body: NewMessage) -> Result<MessageView, ApiError>;
    async fn edit_message(&self, claim: Claim, body: MessageEdit) -> Result<MessageView, ApiError>;
    async fn delete_message(&self, claim: Claim, guild_id: String, channel_id: String, message_id: String) -> Result<(), ApiError>;
    /// Newest first, with translations for the user's language and English.
    async fn get_messages(&self, claim: Claim, guild_id: String, channel_id: String, before: Option<String>, limit: i64) -> Result<Vec<MessageView>, ApiError>;
}

/// Events are delivered by long polling for now. A WebSocket transport will
/// carry the same Event type.
#[async_trait]
pub trait Gateway: Send + Sync + 'static {
    async fn connect(&self, claim: Claim) -> Result<Ready, ApiError>;
    async fn poll(&self, claim: Claim, session_id: String, after: i64, wait_ms: i64) -> Result<EventBatch, ApiError>;
}

