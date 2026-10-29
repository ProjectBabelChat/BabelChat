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
    /// A guild's own emoji, usable and seen in every guild (not yet).
    #[serde(rename = "emoji")]
    Emoji,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum MediaError {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    /// The file is larger than the service takes (10 MB).
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



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait Media: Send + Sync + 'static {
    async fn get_blob(&self, guild_id: String, channel_id: Option<String>, id: String) -> Result<Blob, MediaError>;
    /// Messages took these in: they stay until deleted. Unknown blobs are
    /// skipped; marking twice changes nothing.
    async fn attach_blobs(&self, body: BlobRefs) -> Result<(), MediaError>;
    /// Unknown blobs are skipped.
    async fn delete_blobs(&self, body: BlobRefs) -> Result<(), MediaError>;
    /// Every blob of a channel (a deleted channel or thread).
    async fn delete_channel(&self, guild_id: String, channel_id: String) -> Result<(), MediaError>;
    /// Every blob of a guild (a deleted guild).
    async fn delete_guild(&self, guild_id: String) -> Result<(), MediaError>;
    /// Blobs uploaded before `before` (ms since the epoch), oldest first,
    /// `kind` only if given. For maintenance tools.
    async fn list_blobs(&self, before: i64, kind: Option<BlobKind>, after: Option<String>, limit: i64) -> Result<BlobPage, MediaError>;
    async fn get_usage(&self) -> Result<MediaUsage, MediaError>;
}

