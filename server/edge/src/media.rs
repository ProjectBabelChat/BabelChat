//! Files in and out, next to the JSON API (TYML has no type for them).
//!
//! ```text
//! POST /media/upload?guild_id=&channel_id=&name=   body: the file -> Attachment
//! GET  /media/<guild>/<channel>/<attachment id>    -> the file
//! ```
//!
//! An upload is checked like a message (the user must be allowed to write
//! in the guild, and is rate-limited), passed on to the guild's Media
//! service as it arrives, and recorded on the guild's Message Cell, which
//! hands it to the user's next message there. A download is for the
//! guild's members; whether a user may see a guild is remembered for a
//! minute. Images come back as themselves, anything else as a download,
//! never sniffed by browsers.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use babel_api_edge::types::{self as public, ApiError, Claim};
use babel_api_message_cell::types::{self as cell, Permission};
use babel_media::{BlobKind, BlobRef, BlobRefs, Blobs, MAX_FILE_BYTES, NewBlob, sniff};
use futures::StreamExt;
use serde::Deserialize;

use crate::{ApiResult, Edge, Inner, convert};

/// How long a user found allowed to see a guild may download its files
/// without asking its cell again.
const VIEWER_TTL: Duration = Duration::from_secs(60);
const MAX_NAME_CHARS: usize = 255;

#[derive(Deserialize)]
struct Upload {
    guild_id: String,
    channel_id: String,
    name: String,
}

pub fn router(edge: Edge) -> Router {
    Router::new()
        .route("/media/upload", post(upload))
        .route("/media/{guild_id}/{channel_id}/{id}", get(download))
        .with_state(edge)
}

fn failed(error: ApiError) -> Response {
    let status = match error {
        ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(error)).into_response()
}

async fn upload(
    State(edge): State<Edge>,
    headers: HeaderMap,
    Query(upload): Query<Upload>,
    body: Body,
) -> Response {
    let Some(claim) = edge.bearer(&headers) else {
        return failed(ApiError::Unauthorized);
    };
    match edge.inner.upload(claim, &headers, upload, body).await {
        Ok(attachment) => Json(attachment).into_response(),
        Err(error) => failed(error),
    }
}

async fn download(
    State(edge): State<Edge>,
    headers: HeaderMap,
    Path((guild, channel, id)): Path<(String, String, String)>,
) -> Response {
    let Some(claim) = edge.bearer(&headers) else {
        return failed(ApiError::Unauthorized);
    };
    match edge.inner.download(claim, guild, channel, id).await {
        Ok(response) => response,
        Err(error) => failed(error),
    }
}

/// The name a file is shown and saved under: the last part of a path, no
/// control characters.
fn file_name(name: &str) -> Option<String> {
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let name = name.trim();
    (!name.is_empty() && name != "." && name != ".." && name.chars().count() <= MAX_NAME_CHARS)
        .then(|| name.to_string())
}

/// `Content-Disposition` for `name`: ASCII for old clients, the real name
/// percent-encoded (RFC 6266) for the rest.
fn disposition(kind: &str, name: &str) -> HeaderValue {
    let ascii = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let encoded = name
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<String>();
    HeaderValue::from_str(&format!(
        "{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or(HeaderValue::from_static("attachment"))
}

impl Inner {
    /// The Media service that takes a guild's new files: one per guild,
    /// so a guild's files tend to stay together.
    fn media_for(&self, guild: &str) -> Option<&Arc<dyn Blobs>> {
        let mut names = self.cluster.media.keys().collect::<Vec<_>>();
        names.sort();
        let mut hasher = DefaultHasher::new();
        guild.hash(&mut hasher);
        let name = names.get(hasher.finish() as usize % names.len().max(1))?;
        self.cluster.media.get(*name)
    }

    /// Fails unless the user may see the guild, asking its cell at most
    /// once a minute.
    async fn may_view(&self, user: &str, guild: &str) -> ApiResult<()> {
        let key = (user.to_string(), guild.to_string());
        if self
            .viewers
            .lock()
            .get(&key)
            .is_some_and(|at| at.elapsed() < VIEWER_TTL)
        {
            return Ok(());
        }
        on_cell!(self, guild, |cell| cell.check_permission(
            guild.to_string(),
            user.to_string(),
            Permission::View
        ))?;
        let mut viewers = self.viewers.lock();
        if viewers.len() > 100_000 {
            viewers.retain(|_, at| at.elapsed() < VIEWER_TTL);
        }
        viewers.insert(key, Instant::now());
        Ok(())
    }

    async fn upload(
        &self,
        claim: Claim,
        headers: &HeaderMap,
        upload: Upload,
        body: Body,
    ) -> ApiResult<public::Attachment> {
        self.write_allowed(&claim)?;
        let name = file_name(&upload.name).ok_or(ApiError::InvalidRequest)?;
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());
        if declared.is_some_and(|size| size > MAX_FILE_BYTES) {
            return Err(ApiError::LimitExceeded);
        }
        let (guild, channel) = (upload.guild_id, upload.channel_id);
        on_cell!(self, &guild, |cell| cell.check_permission(
            guild.clone(),
            claim.sub.clone(),
            Permission::SendMessages
        ))?;
        let media = self.media_for(&guild).ok_or(ApiError::Unavailable)?;

        let body = body
            .into_data_stream()
            .map(|chunk| chunk.map_err(io::Error::other));
        let blob = media
            .put(
                NewBlob {
                    guild_id: guild.clone(),
                    channel_id: Some(channel.clone()),
                    kind: BlobKind::Attachment,
                    uploader: claim.sub.clone(),
                    name,
                },
                Box::pin(body),
            )
            .await
            .map_err(convert::media_error)?;
        metrics::counter!("babel_edge_uploads_total").increment(1);

        let attachment = cell::Attachment {
            id: blob.id.clone(),
            name: blob.name,
            content_type: blob.content_type,
            size: blob.size,
            width: blob.width,
            height: blob.height,
        };
        let recorded = on_cell!(self, &guild, |cell| cell.add_upload(cell::NewUpload {
            guild_id: guild.clone(),
            channel_id: channel.clone(),
            user_id: claim.sub.clone(),
            attachment: attachment.clone(),
        }));
        if let Err(error) = recorded {
            // Nothing can send it now: no need to wait for the sweeper.
            let _ = media
                .delete_blobs(BlobRefs {
                    blobs: vec![BlobRef {
                        guild_id: guild,
                        channel_id: Some(channel),
                        id: blob.id,
                    }],
                })
                .await;
            return Err(error);
        }
        Ok(convert::attachment(&guild, &channel, &attachment))
    }

    async fn download(
        &self,
        claim: Claim,
        guild: String,
        channel: String,
        id: String,
    ) -> ApiResult<Response> {
        self.may_view(&claim.sub, &guild).await?;
        let media = id
            .split_once('.')
            .and_then(|(name, _)| self.cluster.media.get(name))
            .ok_or(ApiError::NotFound)?;
        let (blob, stream) = media
            .get(&guild, Some(&channel), &id)
            .await
            .map_err(convert::media_error)?;
        if blob.kind != BlobKind::Attachment {
            return Err(ApiError::NotFound);
        }
        let inline = sniff::INLINE.contains(&blob.content_type.as_str());
        let content_type = if inline {
            blob.content_type.as_str()
        } else {
            "application/octet-stream"
        };
        let mut response = Body::from_stream(stream).into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type)
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(blob.size));
        headers.insert(
            header::CONTENT_DISPOSITION,
            disposition(if inline { "inline" } else { "attachment" }, &blob.name),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        // A file never changes under its ID.
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=31536000, immutable"),
        );
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_paths_and_control_characters() {
        assert_eq!(file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(file_name("C:\\photos\\cat.png").as_deref(), Some("cat.png"));
        assert_eq!(file_name("a\nb.txt").as_deref(), Some("ab.txt"));
        assert_eq!(file_name(".."), None);
        assert_eq!(file_name("  "), None);
        assert_eq!(
            disposition("attachment", "写真 1.png"),
            "attachment; filename=\"__ 1.png\"; filename*=UTF-8''%E5%86%99%E7%9C%9F%201.png"
        );
    }
}
