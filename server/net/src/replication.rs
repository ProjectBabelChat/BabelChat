//! Standbys: a second copy of a service's store, kept a moment behind.
//!
//! A standby is the same service on another machine, with the same
//! certificate. It starts from a checkpoint of the primary's store, then
//! replays the primary's writes as they happen (RocksDB's write-ahead log,
//! batch by batch), so it holds the same data, up to a moment ago, in the
//! same form. Every service replicates the same way; none of them knows.
//!
//! ```text
//! GET /replication/checkpoint             the store's files, once
//! GET /replication/updates?after=&wait_ms=  writes after a sequence number (long poll)
//! ```
//!
//! Only the service itself may call these routes (a standby holds its
//! certificate). Failing over is starting the service on the standby's data
//! directory and pointing the service's address at it; see
//! docs/deployment.md.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use babel_common::Identity;
use babel_store::{RocksStore, Update};
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::Policy;

const MAX_BATCH_BYTES: usize = 4 << 20;
const WAIT_STEP: Duration = Duration::from_millis(50);
const LATEST: &str = "x-babel-latest";

impl Policy {
    /// `self`, plus the replication routes for `service` itself (its
    /// standbys hold its certificate).
    pub fn with_replication(self, service: &Identity) -> Policy {
        let service = service.clone();
        Policy::new(move |caller, path| {
            if path.starts_with("/replication/") {
                *caller == service
            } else {
                self.allows(caller, path)
            }
        })
    }
}

/// The replication routes of a service whose store is `store`.
pub fn router(store: Arc<RocksStore>) -> Router {
    Router::new()
        .route("/replication/updates", get(updates))
        .route("/replication/checkpoint", get(checkpoint))
        .with_state(store)
}

#[derive(Deserialize)]
struct UpdatesQuery {
    after: u64,
    wait_ms: u64,
}

/// Frames: `seq: u64`, `len: u32`, then the batch; all big-endian.
fn encode(updates: &[Update]) -> Vec<u8> {
    let mut body = Vec::new();
    for update in updates {
        body.extend_from_slice(&update.seq.to_be_bytes());
        body.extend_from_slice(&(update.data.len() as u32).to_be_bytes());
        body.extend_from_slice(&update.data);
    }
    body
}

fn decode(mut body: &[u8]) -> anyhow::Result<Vec<Update>> {
    let mut updates = Vec::new();
    while !body.is_empty() {
        if body.len() < 12 {
            bail!("truncated update");
        }
        let seq = u64::from_be_bytes(body[..8].try_into()?);
        let len = u32::from_be_bytes(body[8..12].try_into()?) as usize;
        body = &body[12..];
        if body.len() < len {
            bail!("truncated update");
        }
        updates.push(Update {
            seq,
            data: body[..len].to_vec(),
        });
        body = &body[len..];
    }
    Ok(updates)
}

async fn updates(
    State(store): State<Arc<RocksStore>>,
    Query(query): Query<UpdatesQuery>,
) -> Response {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(query.wait_ms.min(30_000));
    while store.latest_sequence() <= query.after && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(WAIT_STEP).await;
    }
    let reader = store.clone();
    let updates =
        tokio::task::spawn_blocking(move || reader.updates_since(query.after, MAX_BATCH_BYTES))
            .await;
    match updates {
        Ok(Ok(updates)) => {
            let mut response = encode(&updates).into_response();
            response
                .headers_mut()
                .insert(LATEST, HeaderValue::from(store.latest_sequence()));
            response
        }
        // The log no longer goes back that far: the standby starts over.
        Ok(Err(error)) => {
            tracing::warn!(%error, after = query.after, "a standby fell behind the log");
            StatusCode::GONE.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Streams a fresh checkpoint: per file, `name_len: u16`, the name,
/// `size: u64`, the bytes; a zero `name_len` ends it.
async fn checkpoint(State(store): State<Arc<RocksStore>>) -> Response {
    // Next to the store, so the checkpoint can hard-link its files.
    let dir = store
        .path()
        .with_extension(format!("checkpoint-{}", babel_common::random_id()));
    let path = dir.clone();
    let made = tokio::task::spawn_blocking(move || store.checkpoint(&path)).await;
    if !matches!(made, Ok(Ok(()))) {
        tracing::error!(?made, "making a checkpoint failed");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    tokio::spawn(async move {
        let result = send_files(&dir, &sender).await;
        if let Err(error) = result {
            let _ = sender.send(Err(error)).await;
        }
        let _ = tokio::fs::remove_dir_all(&dir).await;
    });
    let stream = futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|chunk| (chunk, receiver))
    });
    Response::new(Body::from_stream(stream))
}

const CHUNK: usize = 1 << 20;

async fn send_files(
    dir: &Path,
    sender: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> std::io::Result<()> {
    let closed = || std::io::Error::other("the standby went away");
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut file = tokio::fs::File::open(entry.path()).await?;
        let size = file.metadata().await?.len();
        let mut head = Vec::new();
        head.extend_from_slice(&(name.len() as u16).to_be_bytes());
        head.extend_from_slice(name.as_bytes());
        head.extend_from_slice(&size.to_be_bytes());
        sender.send(Ok(head.into())).await.map_err(|_| closed())?;
        let mut left = size;
        while left > 0 {
            let mut chunk = vec![0; CHUNK.min(left as usize)];
            file.read_exact(&mut chunk).await?;
            left -= chunk.len() as u64;
            sender.send(Ok(chunk.into())).await.map_err(|_| closed())?;
        }
    }
    sender
        .send(Ok(Bytes::from_static(&[0, 0])))
        .await
        .map_err(|_| closed())
}

/// Reads a checkpoint stream into `dir`.
async fn receive_files(response: reqwest::Response, dir: &Path) -> anyhow::Result<()> {
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::<u8>::new();
    // Takes `n` bytes, reading more as needed; `None` at the end.
    async fn take(
        stream: &mut (impl futures::Stream<Item = reqwest::Result<Bytes>> + Unpin),
        buffer: &mut Vec<u8>,
        n: usize,
    ) -> anyhow::Result<Vec<u8>> {
        while buffer.len() < n {
            match stream.next().await {
                Some(chunk) => buffer.extend_from_slice(&chunk?),
                None => bail!("the checkpoint ended early"),
            }
        }
        Ok(buffer.drain(..n).collect())
    }
    loop {
        let name_len =
            u16::from_be_bytes(take(&mut stream, &mut buffer, 2).await?.try_into().unwrap());
        if name_len == 0 {
            return Ok(());
        }
        let name = String::from_utf8(take(&mut stream, &mut buffer, name_len as usize).await?)?;
        if name.contains(['/', '\\']) || name.starts_with('.') {
            bail!("unexpected file {name:?} in the checkpoint");
        }
        let size = u64::from_be_bytes(take(&mut stream, &mut buffer, 8).await?.try_into().unwrap());
        let mut file = tokio::fs::File::create(dir.join(&name)).await?;
        let mut left = size;
        while left > 0 {
            if buffer.is_empty() {
                match stream.next().await {
                    Some(chunk) => buffer.extend_from_slice(&chunk?),
                    None => bail!("the checkpoint ended early"),
                }
            }
            let n = buffer.len().min(left as usize);
            tokio::io::AsyncWriteExt::write_all(&mut file, &buffer[..n]).await?;
            buffer.drain(..n);
            left -= n as u64;
        }
        file.sync_all().await?;
    }
}

/// Keeps `data` a copy of the store of the service at `url`, until the
/// process ends or the copy cannot continue. An empty `data` starts from a
/// checkpoint. `client` must present the service's own certificate.
pub async fn follow(url: &str, client: &reqwest::Client, data: &Path) -> anyhow::Result<()> {
    let url = url.trim_end_matches('/');
    if !data.join("CURRENT").exists() {
        tracing::info!(%url, "copying a checkpoint of the primary");
        let partial = data.with_extension("partial");
        let _ = tokio::fs::remove_dir_all(&partial).await;
        tokio::fs::create_dir_all(&partial).await?;
        let response = client
            .get(format!("{url}/replication/checkpoint"))
            .timeout(Duration::from_secs(24 * 60 * 60))
            .send()
            .await?
            .error_for_status()?;
        receive_files(response, &partial).await?;
        if data.exists() {
            tokio::fs::remove_dir(data)
                .await
                .with_context(|| format!("{} is in the way", data.display()))?;
        }
        tokio::fs::rename(&partial, data).await?;
    }

    let store = Arc::new(RocksStore::open_existing(data)?);
    tracing::info!(seq = store.latest_sequence(), "following the primary");
    let mut reported = tokio::time::Instant::now();
    loop {
        let after = store.latest_sequence();
        let response = client
            .get(format!("{url}/replication/updates"))
            .query(&[("after", after), ("wait_ms", 25_000)])
            .send()
            .await;
        let response = match response {
            Ok(response) if response.status() == StatusCode::GONE => {
                bail!(
                    "the primary's log no longer reaches back to {after}; empty {} and start again",
                    data.display()
                );
            }
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                tracing::warn!(status = %response.status(), "the primary refused");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "the primary is unreachable");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let latest = latest_of(response.headers());
        let updates = decode(&response.bytes().await?)?;
        let writer = store.clone();
        tokio::task::spawn_blocking(move || {
            updates.iter().try_for_each(|update| writer.apply(update))
        })
        .await??;
        if reported.elapsed() > Duration::from_secs(60) {
            reported = tokio::time::Instant::now();
            let seq = store.latest_sequence();
            tracing::info!(seq, behind = latest.saturating_sub(seq), "standby");
        }
    }
}

fn latest_of(headers: &HeaderMap) -> u64 {
    headers
        .get(LATEST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}

/// Where a standby of the service keeps its copy, if not told otherwise.
pub fn default_data(name: &str) -> PathBuf {
    PathBuf::from("data").join(format!("{name}-standby"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_round_trip() {
        let updates = vec![
            Update {
                seq: 7,
                data: vec![1, 2, 3],
            },
            Update {
                seq: 10,
                data: Vec::new(),
            },
        ];
        assert_eq!(decode(&encode(&updates)).unwrap(), updates);
        assert!(decode(&encode(&updates)[..5]).is_err());
    }
}
