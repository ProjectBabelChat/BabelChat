use std::io;

use async_trait::async_trait;
use babel_api_media::types::{Blob, BlobKind, BlobPage, BlobRefs, MediaError, MediaUsage};
use babel_api_media_client::types::{self as client, ThrowsOrOtherError};
use babel_common::convert;
use futures::StreamExt;

use crate::{Blobs, ByteStream, NewBlob, routes::NO_CHANNEL};

/// A Media service in another process, reached over HTTP. Callers cannot
/// tell it from an in-process [`crate::Media`].
pub struct RemoteMedia {
    client: client::Media,
}

impl RemoteMedia {
    /// `url` is the service's base URL, e.g. `https://10.8.0.7:7130`.
    pub fn new(url: impl Into<String>) -> Self {
        Self::with_client(url, reqwest::Client::new())
    }

    /// Calls the service with `client`, set up for mutual TLS inside a
    /// cluster.
    pub fn with_client(url: impl Into<String>, client: reqwest::Client) -> Self {
        Self {
            client: client::Media {
                url: url.into().trim_end_matches('/').to_string(),
                client,
            },
        }
    }

    /// An answer of the file routes: `T` as JSON, or the error.
    async fn answer<T: serde::de::DeserializeOwned>(
        response: reqwest::Result<reqwest::Response>,
    ) -> Result<T, MediaError> {
        let response = response.map_err(unreachable)?;
        if response.status().is_success() {
            return response.json().await.map_err(unreachable);
        }
        response
            .json()
            .await
            .map_err(unreachable)
            .and_then(|error: MediaError| Err(error))
    }
}

fn unreachable(error: reqwest::Error) -> MediaError {
    tracing::warn!(%error, "media service unreachable");
    MediaError::Unavailable
}

fn media_error(error: ThrowsOrOtherError<client::MediaError>) -> MediaError {
    match error {
        ThrowsOrOtherError::Throws(error) => convert(&error),
        ThrowsOrOtherError::Other(error) => unreachable(error),
    }
}

#[async_trait]
impl babel_api_media::types::Media for RemoteMedia {
    async fn get_blob(
        &self,
        guild_id: String,
        channel_id: Option<String>,
        id: String,
    ) -> Result<Blob, MediaError> {
        self.client
            .get_blob(guild_id, channel_id, id)
            .await
            .map(|blob| convert(&blob))
            .map_err(media_error)
    }

    async fn attach_blobs(&self, body: BlobRefs) -> Result<(), MediaError> {
        self.client
            .attach_blobs(convert(&body))
            .await
            .map_err(media_error)
    }

    async fn delete_blobs(&self, body: BlobRefs) -> Result<(), MediaError> {
        self.client
            .delete_blobs(convert(&body))
            .await
            .map_err(media_error)
    }

    async fn delete_channel(&self, guild_id: String, channel_id: String) -> Result<(), MediaError> {
        self.client
            .delete_channel(guild_id, channel_id)
            .await
            .map_err(media_error)
    }

    async fn delete_guild(&self, guild_id: String) -> Result<(), MediaError> {
        self.client
            .delete_guild(guild_id)
            .await
            .map_err(media_error)
    }

    async fn list_blobs(
        &self,
        before: i64,
        kind: Option<BlobKind>,
        after: Option<String>,
        limit: i64,
    ) -> Result<BlobPage, MediaError> {
        self.client
            .list_blobs(before, kind.map(|kind| convert(&kind)), after, limit)
            .await
            .map(|page| convert(&page))
            .map_err(media_error)
    }

    async fn get_usage(&self) -> Result<MediaUsage, MediaError> {
        self.client
            .get_usage()
            .await
            .map(|usage| convert(&usage))
            .map_err(media_error)
    }
}

#[async_trait]
impl Blobs for RemoteMedia {
    async fn put(&self, blob: NewBlob, body: ByteStream) -> Result<Blob, MediaError> {
        let request = self
            .client
            .client
            .put(format!("{}/media/blobs", self.client.url))
            .query(&blob)
            .body(reqwest::Body::wrap_stream(body));
        Self::answer(request.send().await).await
    }

    async fn get(
        &self,
        guild_id: &str,
        channel_id: Option<&str>,
        id: &str,
    ) -> Result<(Blob, ByteStream), MediaError> {
        use babel_api_media::types::Media;

        let blob = self
            .get_blob(guild_id.into(), channel_id.map(Into::into), id.into())
            .await?;
        let response = self
            .client
            .client
            .get(format!(
                "{}/media/blobs/{guild_id}/{}/{id}",
                self.client.url,
                channel_id.unwrap_or(NO_CHANNEL)
            ))
            .send()
            .await
            .map_err(unreachable)?;
        if !response.status().is_success() {
            return Err(response
                .json::<MediaError>()
                .await
                .unwrap_or(MediaError::Unavailable));
        }
        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(io::Error::other));
        Ok((blob, Box::pin(stream)))
    }
}
