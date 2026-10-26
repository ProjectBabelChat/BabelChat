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
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum AccountError {
    #[serde(rename = "name_taken")]
    NameTaken,
    #[serde(rename = "invalid_credentials")]
    InvalidCredentials,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "not_found")]
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Credentials {
    pub name: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
}

/// Personal settings. They follow the user across guilds and, through the
/// SDK, across platforms.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct UserSettings {
    pub locale: String,
    /// Translate on device even languages a guild does not declare.
    pub translate_outside_set: bool,
    /// Show the original text next to translations.
    pub show_original: bool,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait Account: Send + Sync + 'static {
    async fn register(&self, body: Registration) -> Result<Token, AccountError>;
    async fn login(&self, body: Credentials) -> Result<Token, AccountError>;
    async fn get_user(&self, user_id: String) -> Result<User, AccountError>;
    async fn get_settings(&self, user_id: String) -> Result<UserSettings, AccountError>;
    async fn put_settings(&self, body: UserSettings, user_id: String) -> Result<UserSettings, AccountError>;
}

