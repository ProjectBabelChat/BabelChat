//! The Media service: the files people upload.
//!
//! ```text
//! Edge ──PUT /media/blobs──▶ Media ──▶ blobs     guild / channel / id / chunk  (1 MB each, blob files)
//!      ◀─GET /media/blobs──        ──▶ meta      guild / channel / id          Blob
//! Message Cell ─attach, delete─▶   ──▶ by_time   uploaded at / guild / channel / id
//!                                  ──▶ pending   uploaded at / guild / channel / id  (not taken in yet)
//! ```
//!
//! Files are stored in the service's RocksDB store like any other data, so
//! standbys, backups and failover work as for every service. Their chunks
//! live in blob files beside the LSM tree (`RocksStore::open_with_blobs`),
//! which compactions do not rewrite.
//!
//! [`Blobs`] carries the files themselves, which TYML cannot describe:
//! this struct in-process, [`RemoteMedia`] over HTTP (`routes`).
//!
//! See docs/architecture/media.md.

mod remote;
mod routes;
pub mod sniff;

use std::{io, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use babel_api_media::types::Media as MediaApi;
pub use babel_api_media::types::{
    Blob, BlobKind, BlobPage, BlobRef, BlobRefs, MediaError, MediaUsage,
};
use babel_common::{now_ms, random_id};
use babel_store::{Batch, Key, KvStore, Scan, StoreError, Table};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
pub use remote::RemoteMedia;
pub use routes::router;
use serde::{Deserialize, Serialize};

pub const BLOBS: Table = "blobs";
pub const META: Table = "meta";
pub const BY_TIME: Table = "by_time";
pub const PENDING: Table = "pending";

pub const TABLES: &[Table] = &[BLOBS, META, BY_TIME, PENDING];
/// The tables holding file contents, kept in blob files.
pub const BLOB_TABLES: &[Table] = &[BLOBS];

/// The largest file taken.
pub const MAX_FILE_BYTES: i64 = 10 * 1024 * 1024;
/// Files are stored in pieces of this size, read back one at a time.
const CHUNK_BYTES: usize = 1024 * 1024;
/// Files no message took in are deleted after this long.
pub const UNATTACHED_FOR: Duration = Duration::from_secs(2 * 24 * 60 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
const PAGE: usize = 500;
const MAX_LIST: i64 = 1000;

/// A file's contents, as they arrive or leave.
pub type ByteStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// A file on its way in: everything about it but what it holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBlob {
    pub guild_id: String,
    pub channel_id: Option<String>,
    pub kind: BlobKind,
    pub uploader: String,
    pub name: String,
}

/// File contents in and out of a Media service, next to its API.
#[async_trait]
pub trait Blobs: MediaApi {
    /// Stores the file `body` holds, up to [`MAX_FILE_BYTES`]
    /// (`limit_exceeded`). Empty files are `invalid_request`.
    async fn put(&self, blob: NewBlob, body: ByteStream) -> Result<Blob, MediaError>;

    /// A file and its record.
    async fn get(
        &self,
        guild_id: &str,
        channel_id: Option<&str>,
        id: &str,
    ) -> Result<(Blob, ByteStream), MediaError>;
}

type MediaResult<T> = Result<T, MediaError>;

fn storage(error: StoreError) -> MediaError {
    tracing::error!(%error, "media storage failed");
    MediaError::Unavailable
}

fn blob_key(guild: &str, channel: Option<&str>, id: &str) -> Vec<u8> {
    Key::new()
        .str(guild)
        .str(channel.unwrap_or(""))
        .str(id)
        .build()
}

fn chunk_key(guild: &str, channel: Option<&str>, id: &str, chunk: u64) -> Vec<u8> {
    Key::new()
        .str(guild)
        .str(channel.unwrap_or(""))
        .str(id)
        .u64(chunk)
        .build()
}

/// Where a blob is in the time-ordered indexes (`by_time`, `pending`).
fn time_key(blob: &Blob) -> Vec<u8> {
    Key::new()
        .u64(blob.created_at.max(0) as u64)
        .str(&blob.guild_id)
        .str(blob.channel_id.as_deref().unwrap_or(""))
        .str(&blob.id)
        .build()
}

fn reference(blob: &Blob) -> BlobRef {
    BlobRef {
        guild_id: blob.guild_id.clone(),
        channel_id: blob.channel_id.clone(),
        id: blob.id.clone(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Every Media service of the cluster file, by name, as the services that
/// call them hold them.
pub fn connect(node: &babel_net::Node) -> anyhow::Result<Vec<(String, Arc<RemoteMedia>)>> {
    node.cluster
        .media
        .iter()
        .map(|service| {
            let peer = babel_common::Identity {
                role: babel_common::Role::Media,
                name: service.name.clone(),
            };
            let client = node.http_client(&peer)?;
            Ok((
                service.name.clone(),
                Arc::new(RemoteMedia::with_client(service.url.clone(), client)),
            ))
        })
        .collect()
}

#[derive(Clone)]
pub struct Media {
    inner: Arc<Inner>,
}

struct Inner {
    /// This service's name, which begins every blob ID it gives.
    name: String,
    store: Arc<dyn KvStore>,
}

impl Media {
    /// Opens the service and starts deleting files no message took in.
    /// Must be called within a Tokio runtime.
    pub fn open(name: impl Into<String>, store: Arc<dyn KvStore>) -> Self {
        let media = Self {
            inner: Arc::new(Inner {
                name: name.into(),
                store,
            }),
        };
        let sweeper = Arc::downgrade(&media.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(SWEEP_EVERY).await;
                let Some(inner) = sweeper.upgrade() else {
                    return;
                };
                match inner.sweep(UNATTACHED_FOR) {
                    Ok(0) => {}
                    Ok(blobs) => tracing::info!(blobs, "deleted files no message took in"),
                    Err(error) => tracing::error!(%error, "deleting unused files failed"),
                }
            }
        });
        media
    }

    /// Deletes the files no message took in for `unattached_for` now (it
    /// also happens every hour, after two days). Returns how many went.
    pub fn sweep(&self, unattached_for: Duration) -> Result<usize, StoreError> {
        self.inner.sweep(unattached_for)
    }
}

impl Inner {
    fn record(&self, guild: &str, channel: Option<&str>, id: &str) -> MediaResult<Blob> {
        self.store
            .get_value::<Blob>(META, &blob_key(guild, channel, id))
            .map_err(storage)?
            .ok_or(MediaError::NotFound)
    }

    fn delete(&self, batch: &mut Batch, blob: &Blob) {
        let channel = blob.channel_id.as_deref();
        batch.delete(META, blob_key(&blob.guild_id, channel, &blob.id));
        batch.delete_prefix(BLOBS, blob_key(&blob.guild_id, channel, &blob.id));
        batch.delete(BY_TIME, time_key(blob));
        batch.delete(PENDING, time_key(blob));
    }

    async fn put(&self, new: NewBlob, mut body: ByteStream) -> MediaResult<Blob> {
        if new.name.is_empty() || new.name.chars().count() > 255 || new.guild_id.is_empty() {
            return Err(MediaError::InvalidRequest);
        }
        if (new.kind == BlobKind::Attachment) != new.channel_id.is_some() {
            return Err(MediaError::InvalidRequest);
        }
        let mut blob = Blob {
            id: format!("{}.{}", self.name, random_id()),
            guild_id: new.guild_id,
            channel_id: new.channel_id,
            kind: new.kind,
            uploader: new.uploader,
            name: new.name,
            content_type: "application/octet-stream".into(),
            size: 0,
            width: None,
            height: None,
            created_at: now_ms(),
            attached_at: None,
        };
        // The record first, as pending: should the process stop halfway,
        // the sweeper finds the chunks through it.
        let mut batch = Batch::new();
        let key = blob_key(&blob.guild_id, blob.channel_id.as_deref(), &blob.id);
        batch.put_value(META, key.clone(), &blob);
        batch.put_value(BY_TIME, time_key(&blob), &reference(&blob));
        batch.put_value(PENDING, time_key(&blob), &reference(&blob));
        self.store.write(batch).map_err(storage)?;

        let stored = self.store_chunks(&mut blob, &mut body).await;
        if let Err(error) = stored {
            let mut batch = Batch::new();
            self.delete(&mut batch, &blob);
            self.store.write(batch).map_err(storage)?;
            return Err(error);
        }
        let mut batch = Batch::new();
        batch.put_value(META, key, &blob);
        self.store.write(batch).map_err(storage)?;
        metrics::counter!("babel_media_uploaded_bytes_total").increment(blob.size as u64);
        Ok(blob)
    }

    /// Reads `body` into chunks, recording size and type in `blob`.
    async fn store_chunks(&self, blob: &mut Blob, body: &mut ByteStream) -> MediaResult<()> {
        let channel = blob.channel_id.clone();
        let mut buffer = BytesMut::with_capacity(CHUNK_BYTES);
        let mut chunk = 0u64;
        let mut ended = false;
        while !ended {
            match body.next().await {
                Some(Ok(bytes)) => buffer.extend_from_slice(&bytes),
                Some(Err(error)) => {
                    tracing::debug!(%error, "upload ended early");
                    return Err(MediaError::InvalidRequest);
                }
                None => ended = true,
            }
            if blob.size + buffer.len() as i64 > MAX_FILE_BYTES {
                return Err(MediaError::LimitExceeded);
            }
            while buffer.len() >= CHUNK_BYTES || (ended && !buffer.is_empty()) {
                let piece = buffer.split_to(buffer.len().min(CHUNK_BYTES));
                if chunk == 0 {
                    blob.content_type = sniff::content_type(&piece).into();
                    if let Some((width, height)) = sniff::dimensions(&blob.content_type, &piece) {
                        blob.width = Some(width);
                        blob.height = Some(height);
                    }
                }
                blob.size += piece.len() as i64;
                let mut batch = Batch::new();
                batch.put(
                    BLOBS,
                    chunk_key(&blob.guild_id, channel.as_deref(), &blob.id, chunk),
                    piece.to_vec(),
                );
                self.store.write(batch).map_err(storage)?;
                chunk += 1;
            }
        }
        if blob.size == 0 {
            return Err(MediaError::InvalidRequest);
        }
        Ok(())
    }

    fn get(&self, guild: &str, channel: Option<&str>, id: &str) -> MediaResult<(Blob, ByteStream)> {
        let blob = self.record(guild, channel, id)?;
        // A record whose chunks are still arriving.
        if blob.size == 0 {
            return Err(MediaError::NotFound);
        }
        let store = self.store.clone();
        let (guild, channel, id) = (
            guild.to_string(),
            channel.map(str::to_string),
            id.to_string(),
        );
        let chunks = futures::stream::iter(0u64..)
            .map(move |chunk| {
                store
                    .get(BLOBS, &chunk_key(&guild, channel.as_deref(), &id, chunk))
                    .map_err(io::Error::other)
            })
            .take_while(|chunk| std::future::ready(!matches!(chunk, Ok(None))))
            .map(|chunk| chunk.map(|bytes| Bytes::from(bytes.unwrap_or_default())));
        Ok((blob, Box::pin(chunks)))
    }

    fn attach(&self, refs: BlobRefs) -> MediaResult<()> {
        let now = now_ms();
        let mut batch = Batch::new();
        for blob in refs.blobs {
            let key = blob_key(&blob.guild_id, blob.channel_id.as_deref(), &blob.id);
            let Some(mut blob) = self.store.get_value::<Blob>(META, &key).map_err(storage)? else {
                continue;
            };
            if blob.attached_at.is_some() {
                continue;
            }
            blob.attached_at = Some(now);
            batch.delete(PENDING, time_key(&blob));
            batch.put_value(META, key, &blob);
        }
        self.store.write(batch).map_err(storage)
    }

    fn delete_blobs(&self, refs: BlobRefs) -> MediaResult<()> {
        let mut batch = Batch::new();
        for blob in refs.blobs {
            match self.record(&blob.guild_id, blob.channel_id.as_deref(), &blob.id) {
                Ok(blob) => self.delete(&mut batch, &blob),
                Err(MediaError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        self.store.write(batch).map_err(storage)
    }

    /// Deletes everything under `prefix` of `meta` and `blobs`. Their
    /// entries in the time indexes go when next met.
    fn delete_prefix(&self, prefix: Vec<u8>) -> MediaResult<()> {
        let mut batch = Batch::new();
        batch.delete_prefix(META, prefix.clone());
        batch.delete_prefix(BLOBS, prefix);
        self.store.write(batch).map_err(storage)
    }

    /// Walks a time index from its oldest entry, up to `before`, giving
    /// each blob still there to `visit` (until it returns false). Entries
    /// of blobs deleted with their channel or guild are dropped.
    fn walk(
        &self,
        table: Table,
        before: i64,
        after: Option<&[u8]>,
        mut visit: impl FnMut(&[u8], Blob) -> bool,
    ) -> Result<(), StoreError> {
        let mut after = after.map(<[u8]>::to_vec);
        loop {
            let mut scan = Scan::prefix(table, &[]).limit(PAGE);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan_values::<BlobRef>(scan)?;
            let Some((last, _)) = page.last() else {
                return Ok(());
            };
            after = Some(last.clone());
            let mut gone = Batch::new();
            for (key, blob) in page {
                let at = babel_store::KeyReader::new(&key).u64().unwrap_or(0) as i64;
                if at >= before {
                    self.store.write(gone)?;
                    return Ok(());
                }
                let key_of = blob_key(&blob.guild_id, blob.channel_id.as_deref(), &blob.id);
                match self.store.get_value::<Blob>(META, &key_of)? {
                    Some(blob) => {
                        if !visit(&key, blob) {
                            self.store.write(gone)?;
                            return Ok(());
                        }
                    }
                    None => gone.delete(table, key),
                }
            }
            self.store.write(gone)?;
        }
    }

    fn sweep(&self, unattached_for: Duration) -> Result<usize, StoreError> {
        let before = now_ms() - unattached_for.as_millis() as i64;
        let mut stale = Vec::new();
        self.walk(PENDING, before, None, |_, blob| {
            stale.push(blob);
            true
        })?;
        let mut batch = Batch::new();
        for blob in &stale {
            self.delete(&mut batch, blob);
        }
        self.store.write(batch)?;
        Ok(stale.len())
    }

    fn list(
        &self,
        before: i64,
        kind: Option<BlobKind>,
        after: Option<String>,
        limit: i64,
    ) -> MediaResult<BlobPage> {
        let after = match after {
            Some(after) => Some(unhex(&after).ok_or(MediaError::InvalidRequest)?),
            None => None,
        };
        let limit = limit.clamp(1, MAX_LIST) as usize;
        let mut blobs = Vec::new();
        let mut next = None;
        self.walk(BY_TIME, before, after.as_deref(), |key, blob| {
            if kind.is_none_or(|kind| kind == blob.kind) {
                blobs.push(blob);
            }
            if blobs.len() == limit {
                next = Some(hex(key));
                return false;
            }
            true
        })
        .map_err(storage)?;
        Ok(BlobPage { blobs, next })
    }

    fn usage(&self) -> MediaResult<MediaUsage> {
        let mut usage = MediaUsage { blobs: 0, bytes: 0 };
        let mut after = None::<Vec<u8>>;
        loop {
            let mut scan = Scan::prefix(META, &[]).limit(PAGE);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan_values::<Blob>(scan).map_err(storage)?;
            let Some((last, _)) = page.last() else {
                return Ok(usage);
            };
            after = Some(last.clone());
            for (_, blob) in page {
                usage.blobs += 1;
                usage.bytes += blob.size;
            }
        }
    }
}

#[async_trait]
impl MediaApi for Media {
    async fn get_blob(
        &self,
        guild_id: String,
        channel_id: Option<String>,
        id: String,
    ) -> MediaResult<Blob> {
        self.inner.record(&guild_id, channel_id.as_deref(), &id)
    }

    async fn attach_blobs(&self, body: BlobRefs) -> MediaResult<()> {
        self.inner.attach(body)
    }

    async fn delete_blobs(&self, body: BlobRefs) -> MediaResult<()> {
        self.inner.delete_blobs(body)
    }

    async fn delete_channel(&self, guild_id: String, channel_id: String) -> MediaResult<()> {
        if guild_id.is_empty() || channel_id.is_empty() {
            return Err(MediaError::InvalidRequest);
        }
        self.inner
            .delete_prefix(Key::new().str(&guild_id).str(&channel_id).build())
    }

    async fn delete_guild(&self, guild_id: String) -> MediaResult<()> {
        if guild_id.is_empty() {
            return Err(MediaError::InvalidRequest);
        }
        self.inner.delete_prefix(Key::new().str(&guild_id).build())
    }

    async fn list_blobs(
        &self,
        before: i64,
        kind: Option<BlobKind>,
        after: Option<String>,
        limit: i64,
    ) -> MediaResult<BlobPage> {
        self.inner.list(before, kind, after, limit)
    }

    async fn get_usage(&self) -> MediaResult<MediaUsage> {
        self.inner.usage()
    }
}

#[async_trait]
impl Blobs for Media {
    async fn put(&self, blob: NewBlob, body: ByteStream) -> MediaResult<Blob> {
        self.inner.put(blob, body).await
    }

    async fn get(
        &self,
        guild_id: &str,
        channel_id: Option<&str>,
        id: &str,
    ) -> MediaResult<(Blob, ByteStream)> {
        self.inner.get(guild_id, channel_id, id)
    }
}
