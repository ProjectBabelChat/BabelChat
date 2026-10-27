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
pub enum AccountError {
    #[serde(rename = "name_taken")]
    NameTaken,
    #[serde(rename = "invalid_credentials")]
    InvalidCredentials,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "not_found")]
    NotFound,
    /// The Account service could not be reached.
    #[serde(rename = "unavailable")]
    Unavailable,
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



pub struct Account {
    pub url: String,
    pub client: reqwest::Client,
}

impl Account {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn register(&self, __body: Registration) -> Result<Token, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/register", &self.url));
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
    pub async fn login(&self, __body: Credentials) -> Result<Token, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/login", &self.url));
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
    pub async fn get_user(&self, user_id: String) -> Result<User, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.get(format!("{}/account/get_user", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap())]);
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
    pub async fn get_settings(&self, user_id: String) -> Result<UserSettings, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.get(format!("{}/account/get_settings", &self.url));
        request = request.query(&[("user_id", serde_json::to_string(&user_id).unwrap())]);
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
    pub async fn put_settings(&self, __body: UserSettings, user_id: String) -> Result<UserSettings, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/put_settings", &self.url));
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
}
