use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ThrowsOrOtherError<T> {
    Throws(T),
    Other(reqwest::Error),
}

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



pub struct Auth {
    pub url: String,
    pub client: reqwest::Client,
}

impl Auth {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn register(&self, __body: Registration) -> Result<Token, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/auth/register", &self.url));
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
    pub async fn login(&self, __body: Credentials) -> Result<Token, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/auth/login", &self.url));
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
}
pub struct Me {
    pub url: String,
    pub client: reqwest::Client,
}

impl Me {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn get_settings(&self, __token: impl Into<String>) -> Result<UserSettings, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.get(format!("{}/me/get_settings", &self.url));
        request = request.bearer_auth(__token.into());
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
    pub async fn put_settings(&self, __token: impl Into<String>, __body: UserSettings) -> Result<UserSettings, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/me/put_settings", &self.url));
        request = request.bearer_auth(__token.into());
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
}
pub struct Guilds {
    pub url: String,
    pub client: reqwest::Client,
}

impl Guilds {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn create_guild(&self, __token: impl Into<String>, __body: NewGuild) -> Result<GuildView, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/create_guild", &self.url));
        request = request.bearer_auth(__token.into());
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
    /// Any member may invite.
    pub async fn create_invite(&self, __token: impl Into<String>, __body: NewInvite) -> Result<Invite, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/create_invite", &self.url));
        request = request.bearer_auth(__token.into());
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
    /// The invite's creator or the owner may revoke it.
    pub async fn revoke_invite(&self, __token: impl Into<String>, code: String) -> Result<(), ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/revoke_invite", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("code", serde_json::to_string(&code).unwrap())]);
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
    /// Joins with an invite code. Invalid, expired and used-up codes are all
    /// `not_found`.
    pub async fn join_guild(&self, __token: impl Into<String>, invite: String) -> Result<GuildView, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/join_guild", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("invite", serde_json::to_string(&invite).unwrap())]);
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
    pub async fn get_guild(&self, __token: impl Into<String>, guild_id: String) -> Result<GuildView, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.get(format!("{}/guilds/get_guild", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap())]);
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
    pub async fn create_channel(&self, __token: impl Into<String>, __body: NewChannel) -> Result<Channel, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/create_channel", &self.url));
        request = request.bearer_auth(__token.into());
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
    /// Only the owner may change the policy. Languages are normalized and
    /// counted after normalization.
    pub async fn put_language_policy(&self, __token: impl Into<String>, __body: LanguagePolicyUpdate) -> Result<LanguagePolicy, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/guilds/put_language_policy", &self.url));
        request = request.bearer_auth(__token.into());
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
}
pub struct Messages {
    pub url: String,
    pub client: reqwest::Client,
}

impl Messages {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn send_message(&self, __token: impl Into<String>, __body: NewMessage) -> Result<MessageView, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/messages/send_message", &self.url));
        request = request.bearer_auth(__token.into());
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
    pub async fn edit_message(&self, __token: impl Into<String>, __body: MessageEdit) -> Result<MessageView, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/messages/edit_message", &self.url));
        request = request.bearer_auth(__token.into());
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
    pub async fn delete_message(&self, __token: impl Into<String>, guild_id: String, channel_id: String, message_id: String) -> Result<(), ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/messages/delete_message", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("message_id", serde_json::to_string(&message_id).unwrap())]);
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
    /// Newest first, with translations for the user's language and English.
    pub async fn get_messages(&self, __token: impl Into<String>, guild_id: String, channel_id: String, before: Option<String>, limit: i64) -> Result<Vec<MessageView>, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.get(format!("{}/messages/get_messages", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("before", serde_json::to_string(&before).unwrap()), ("limit", serde_json::to_string(&limit).unwrap())]);
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
}
/// Events are delivered by long polling for now. A WebSocket transport will
/// carry the same Event type.
pub struct Gateway {
    pub url: String,
    pub client: reqwest::Client,
}

impl Gateway {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn connect(&self, __token: impl Into<String>) -> Result<Ready, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.post(format!("{}/gateway/connect", &self.url));
        request = request.bearer_auth(__token.into());
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
    pub async fn poll(&self, __token: impl Into<String>, session_id: String, after: i64, wait_ms: i64) -> Result<EventBatch, ThrowsOrOtherError<ApiError>> {
        let mut request = self.client.get(format!("{}/gateway/poll", &self.url));
        request = request.bearer_auth(__token.into());
        request = request.query(&[("session_id", serde_json::to_string(&session_id).unwrap()), ("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
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
}
