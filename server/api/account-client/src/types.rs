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
    /// Needed when sign-ups are by invitation only.
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
pub enum AccountError {
    #[serde(rename = "name_taken")]
    NameTaken,
    #[serde(rename = "invalid_credentials")]
    InvalidCredentials,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "not_found")]
    NotFound,
    /// Sign-ups are closed.
    #[serde(rename = "signups_closed")]
    SignupsClosed,
    /// Sign-ups are by invitation, and the code is missing, expired or
    /// used up.
    #[serde(rename = "invalid_signup_code")]
    InvalidSignupCode,
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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PasswordChange {
    pub password: String,
    pub new_password: String,
}

/// Lets people create accounts while sign-ups are by invitation only.
/// Operators make them (`babel-admin signup-code`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct NewSignupCode {
    /// How many accounts it may create.
    pub uses: i64,
    /// How long it stays valid.
    pub hours: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SignupCode {
    pub code: String,
    pub uses_left: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RevocationBatch {
    pub revocations: Vec<Revocation>,
    /// Pass as `after` to the next watch.
    pub version: i64,
}

/// Tokens of `user_id` issued before `not_before` (ms) are no longer valid:
/// the user signed out their other devices or changed their password.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Revocation {
    pub user_id: String,
    pub not_before: i64,
    /// Position in the stream of revocations.
    pub version: i64,
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
    /// Signs the user's other devices out, like `sign_out_others`.
    pub async fn change_password(&self, __body: PasswordChange, user_id: String) -> Result<Token, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/change_password", &self.url));
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
    /// Revokes every token of the user and returns a new one for the
    /// device asking.
    pub async fn sign_out_others(&self, user_id: String) -> Result<Token, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/sign_out_others", &self.url));
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
    /// Operators only.
    pub async fn create_signup_code(&self, __body: NewSignupCode) -> Result<SignupCode, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.post(format!("{}/account/create_signup_code", &self.url));
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
    /// Long poll: revocations after version `after`; from 0, all of those
    /// that still matter (older ones only concern tokens that have expired).
    /// Edges follow it and refuse revoked tokens.
    pub async fn watch_revocations(&self, after: i64, wait_ms: i64) -> Result<RevocationBatch, ThrowsOrOtherError<AccountError>> {
        let mut request = self.client.get(format!("{}/account/watch_revocations", &self.url));
        request = request.query(&[("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
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
