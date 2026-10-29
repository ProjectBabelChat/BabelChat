use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ThrowsOrOtherError<T> {
    Throws(T),
    Other(reqwest::Error),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Blob {
    /// "<media service>.<random>".
    pub id: String,
    pub guild_id: String,
    /// The channel an attachment was sent in; absent for emoji.
    pub channel_id: Option<String>,
    pub kind: BlobKind,
    pub uploader: String,
    /// As the uploader named it.
    pub name: String,
    /// Read from the file itself, never taken from the uploader:
    /// `image/png`, `image/jpeg`, `image/gif`, `image/webp`, a few
    /// others, else `application/octet-stream`.
    pub content_type: String,
    pub size: i64,
    /// Of images.
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: i64,
    /// When a message took it in. Blobs nothing took in within two days
    /// are deleted.
    pub attached_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum BlobKind {
    /// A file sent with a message, seen by the guild's members.
    #[serde(rename = "attachment")]
    Attachment,
    /// A guild's own emoji, used and seen in every guild.
    #[serde(rename = "emoji")]
    Emoji,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum MediaError {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    /// The file is larger than the service takes (10 MB; 256 KB for
    /// emoji).
    #[serde(rename = "limit_exceeded")]
    LimitExceeded,
    #[serde(rename = "unavailable")]
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobRefs {
    pub blobs: Vec<BlobRef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobRef {
    pub guild_id: String,
    pub channel_id: Option<String>,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BlobPage {
    pub blobs: Vec<Blob>,
    /// Pass as `after` for the next page; absent on the last.
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MediaUsage {
    pub blobs: i64,
    pub bytes: i64,
}



pub struct Media {
    pub url: String,
    pub client: reqwest::Client,
}

impl Media {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn get_blob(&self, guild_id: String, channel_id: Option<String>, id: String) -> Result<Blob, ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/get_blob", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap()), ("id", serde_json::to_string(&id).unwrap())]);
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
    /// Messages took these in: they stay until deleted. Unknown blobs are
    /// skipped; marking twice changes nothing.
    pub async fn attach_blobs(&self, __body: BlobRefs) -> Result<(), ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/attach_blobs", &self.url));
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
    /// Unknown blobs are skipped.
    pub async fn delete_blobs(&self, __body: BlobRefs) -> Result<(), ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/delete_blobs", &self.url));
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
    /// Every blob of a channel (a deleted channel or thread).
    pub async fn delete_channel(&self, guild_id: String, channel_id: String) -> Result<(), ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/delete_channel", &self.url));
        request = request.query(&[("guild_id", serde_json::to_string(&guild_id).unwrap()), ("channel_id", serde_json::to_string(&channel_id).unwrap())]);
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
    /// Every blob of a guild (a deleted guild).
    pub async fn delete_guild(&self, guild_id: String) -> Result<(), ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/delete_guild", &self.url));
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
    /// Blobs uploaded before `before` (ms since the epoch), oldest first,
    /// `kind` only if given. For maintenance tools.
    pub async fn list_blobs(&self, before: i64, kind: Option<BlobKind>, after: Option<String>, limit: i64) -> Result<BlobPage, ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/list_blobs", &self.url));
        request = request.query(&[("before", serde_json::to_string(&before).unwrap()), ("kind", serde_json::to_string(&kind).unwrap()), ("after", serde_json::to_string(&after).unwrap()), ("limit", serde_json::to_string(&limit).unwrap())]);
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
    pub async fn get_usage(&self) -> Result<MediaUsage, ThrowsOrOtherError<MediaError>> {
        let mut request = self.client.post(format!("{}/media/get_usage", &self.url));
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
