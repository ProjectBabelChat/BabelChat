//! The Media service in-process and over HTTP.

use std::{sync::Arc, time::Duration};

use babel_api_media::types::Media as _;
use babel_media::{
    BLOB_TABLES, BlobKind, BlobRef, BlobRefs, Blobs, ByteStream, MAX_FILE_BYTES, Media, MediaError,
    NewBlob, RemoteMedia, TABLES,
};
use babel_store::RocksStore;
use bytes::Bytes;
use futures::StreamExt;

fn media(dir: &tempfile::TempDir) -> Media {
    let store = RocksStore::open_with_blobs(dir.path(), TABLES, BLOB_TABLES).unwrap();
    Media::open("media-0", Arc::new(store))
}

fn attachment(channel: &str, name: &str) -> NewBlob {
    NewBlob {
        guild_id: "g".into(),
        channel_id: Some(channel.into()),
        kind: BlobKind::Attachment,
        uploader: "u".into(),
        name: name.into(),
    }
}

/// `bytes` as an upload arriving in pieces of `piece` bytes.
fn body(bytes: &[u8], piece: usize) -> ByteStream {
    let pieces = bytes
        .chunks(piece)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    Box::pin(futures::stream::iter(pieces))
}

async fn read(stream: ByteStream) -> Vec<u8> {
    stream.map(|chunk| chunk.unwrap().to_vec()).concat().await
}

/// A PNG of 300 x 200 pixels, padded to `size` bytes.
fn png(size: usize) -> Vec<u8> {
    let mut png = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
        0, 1, 44, 0, 0, 0, 200, 8, 6, 0, 0, 0,
    ];
    png.extend((0..size - png.len()).map(|i| (i % 251) as u8));
    png
}

#[tokio::test]
async fn files_come_back_as_they_went_in() {
    let dir = tempfile::tempdir().unwrap();
    let media = media(&dir);
    let file = png(2_500_000);
    let blob = media
        .put(attachment("c", "cat.png"), body(&file, 70_000))
        .await
        .unwrap();
    assert!(blob.id.starts_with("media-0."));
    assert_eq!(blob.content_type, "image/png");
    assert_eq!((blob.width, blob.height), (Some(300), Some(200)));
    assert_eq!(blob.size, file.len() as i64);
    assert_eq!(blob.attached_at, None);

    let (record, stream) = media.get("g", Some("c"), &blob.id).await.unwrap();
    assert_eq!(record, blob);
    assert_eq!(read(stream).await, file);
    // Only under its own guild and channel.
    assert!(matches!(
        media.get("g", Some("other"), &blob.id).await,
        Err(MediaError::NotFound)
    ));

    let usage = media.get_usage().await.unwrap();
    assert_eq!((usage.blobs, usage.bytes), (1, file.len() as i64));
}

#[tokio::test]
async fn too_large_and_empty_files_are_refused_and_leave_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let media = media(&dir);
    let file = vec![0u8; MAX_FILE_BYTES as usize + 1];
    assert!(matches!(
        media
            .put(attachment("c", "big.bin"), body(&file, 1 << 20))
            .await,
        Err(MediaError::LimitExceeded)
    ));
    assert!(matches!(
        media.put(attachment("c", "empty.txt"), body(&[], 1)).await,
        Err(MediaError::InvalidRequest)
    ));
    // Attachments belong to a channel.
    let mut no_channel = attachment("c", "a.txt");
    no_channel.channel_id = None;
    assert!(matches!(
        media.put(no_channel, body(b"hi", 1)).await,
        Err(MediaError::InvalidRequest)
    ));
    let usage = media.get_usage().await.unwrap();
    assert_eq!((usage.blobs, usage.bytes), (0, 0));
    let listed = media.list_blobs(i64::MAX, None, None, 10).await.unwrap();
    assert!(listed.blobs.is_empty());
}

#[tokio::test]
async fn files_no_message_took_in_go_and_the_rest_stay() {
    let dir = tempfile::tempdir().unwrap();
    let media = media(&dir);
    let sent = media
        .put(attachment("c", "sent.txt"), body(b"sent", 4))
        .await
        .unwrap();
    let dropped = media
        .put(attachment("c", "dropped.txt"), body(b"dropped", 4))
        .await
        .unwrap();
    let refs = |blob: &babel_media::Blob| BlobRefs {
        blobs: vec![BlobRef {
            guild_id: blob.guild_id.clone(),
            channel_id: blob.channel_id.clone(),
            id: blob.id.clone(),
        }],
    };
    media.attach_blobs(refs(&sent)).await.unwrap();
    media.attach_blobs(refs(&sent)).await.unwrap();

    // Not yet: they are young.
    assert_eq!(media.sweep(Duration::from_secs(60)).unwrap(), 0);
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(media.sweep(Duration::ZERO).unwrap(), 1);
    assert!(media.get("g", Some("c"), &sent.id).await.is_ok());
    assert!(matches!(
        media.get("g", Some("c"), &dropped.id).await,
        Err(MediaError::NotFound)
    ));
    assert!(
        media
            .get_blob("g".into(), Some("c".into()), sent.id.clone())
            .await
            .unwrap()
            .attached_at
            .is_some()
    );

    media.delete_blobs(refs(&sent)).await.unwrap();
    media.delete_blobs(refs(&sent)).await.unwrap();
    assert_eq!(media.get_usage().await.unwrap().blobs, 0);
}

#[tokio::test]
async fn channels_and_guilds_take_their_files_and_listing_walks_by_time() {
    let dir = tempfile::tempdir().unwrap();
    let media = media(&dir);
    let mut ids = Vec::new();
    for (channel, name) in [
        ("a", "1.txt"),
        ("b", "2.txt"),
        ("a", "3.txt"),
        ("b", "4.txt"),
    ] {
        let blob = media
            .put(attachment(channel, name), body(name.as_bytes(), 3))
            .await
            .unwrap();
        ids.push(blob.id);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let mut emoji = attachment("a", "party.png");
    emoji.channel_id = None;
    emoji.kind = BlobKind::Emoji;
    let emoji = media.put(emoji, body(&png(100), 50)).await.unwrap();

    // Oldest first, in pages.
    let first = media.list_blobs(i64::MAX, None, None, 2).await.unwrap();
    assert_eq!(
        first.blobs.iter().map(|blob| &blob.id).collect::<Vec<_>>(),
        [&ids[0], &ids[1]]
    );
    let rest = media
        .list_blobs(i64::MAX, Some(BlobKind::Attachment), first.next, 10)
        .await
        .unwrap();
    assert_eq!(
        rest.blobs.iter().map(|blob| &blob.id).collect::<Vec<_>>(),
        [&ids[2], &ids[3]]
    );
    let emoji_only = media
        .list_blobs(i64::MAX, Some(BlobKind::Emoji), None, 10)
        .await
        .unwrap();
    assert_eq!(emoji_only.blobs.len(), 1);
    assert_eq!(emoji_only.blobs[0].id, emoji.id);

    media.delete_channel("g".into(), "a".into()).await.unwrap();
    let left = media.list_blobs(i64::MAX, None, None, 10).await.unwrap();
    assert_eq!(
        left.blobs.iter().map(|blob| &blob.id).collect::<Vec<_>>(),
        [&ids[1], &ids[3], &emoji.id]
    );
    media.delete_guild("g".into()).await.unwrap();
    assert!(
        media
            .list_blobs(i64::MAX, None, None, 10)
            .await
            .unwrap()
            .blobs
            .is_empty()
    );
    assert_eq!(media.get_usage().await.unwrap().bytes, 0);
}

#[tokio::test]
async fn remote_callers_cannot_tell_the_difference() {
    let dir = tempfile::tempdir().unwrap();
    let media = media(&dir);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, babel_media::router(media))
            .await
            .unwrap()
    });
    let remote = RemoteMedia::new(format!("http://{address}"));

    let file = png(1_300_000);
    let blob = remote
        .put(attachment("c", "名前 with spaces.png"), body(&file, 64_000))
        .await
        .unwrap();
    assert_eq!(blob.name, "名前 with spaces.png");
    assert_eq!(blob.content_type, "image/png");
    let (record, stream) = remote.get("g", Some("c"), &blob.id).await.unwrap();
    assert_eq!(record.id, blob.id);
    assert_eq!(read(stream).await, file);

    assert!(matches!(
        remote.get("g", Some("c"), "media-0.missing").await,
        Err(MediaError::NotFound)
    ));
    let big = vec![1u8; MAX_FILE_BYTES as usize + 10];
    assert!(matches!(
        remote
            .put(attachment("c", "big"), body(&big, 1 << 20))
            .await,
        Err(MediaError::LimitExceeded)
    ));
    assert_eq!(remote.get_usage().await.unwrap().blobs, 1);
}
