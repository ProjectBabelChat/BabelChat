//! The Media API over HTTP: the generated routes, and two for file
//! contents, which TYML cannot describe.
//!
//! ```text
//! PUT /media/blobs?guild_id=&channel_id=&kind=&uploader=&name=   body: the file -> Blob
//! GET /media/blobs/<guild_id>/<channel_id or "-">/<id>            -> the file
//! ```
//!
//! Errors are `MediaError` as JSON with 400, like the generated routes'.

use std::io;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use futures::StreamExt;

use crate::{Blobs, Media, MediaError, NewBlob};

/// Stands for "no channel" in a file's path.
pub(crate) const NO_CHANNEL: &str = "-";

pub fn router(media: Media) -> Router {
    let files = Router::new()
        .route("/media/blobs", put(upload))
        .route("/media/blobs/{guild_id}/{channel_id}/{id}", get(download))
        .with_state(media.clone());
    babel_api_media::router(media).merge(files)
}

fn failed(error: MediaError) -> Response {
    (StatusCode::BAD_REQUEST, Json(error)).into_response()
}

async fn upload(State(media): State<Media>, Query(blob): Query<NewBlob>, body: Body) -> Response {
    let stream = body
        .into_data_stream()
        .map(|chunk| chunk.map_err(io::Error::other));
    match media.put(blob, Box::pin(stream)).await {
        Ok(blob) => Json(blob).into_response(),
        Err(error) => failed(error),
    }
}

async fn download(
    State(media): State<Media>,
    Path((guild_id, channel_id, id)): Path<(String, String, String)>,
) -> Response {
    let channel_id = (channel_id != NO_CHANNEL).then_some(channel_id);
    match media.get(&guild_id, channel_id.as_deref(), &id).await {
        Ok((blob, stream)) => (
            [
                (header::CONTENT_TYPE, blob.content_type),
                (header::CONTENT_LENGTH, blob.size.to_string()),
            ],
            Body::from_stream(stream),
        )
            .into_response(),
        Err(error) => failed(error),
    }
}
