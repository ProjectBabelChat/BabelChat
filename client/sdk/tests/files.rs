//! Files sent with messages, through the SDK against a whole in-process
//! server.

mod common;

use babel_api_translation_cell::types::Plan;
use babel_sdk::{ApiError, Error, MAX_UPLOAD_BYTES, SessionConfig, Shown};
use common::{PASSWORD, connect, guild, server, user};

/// A PNG of 64 x 48 pixels, padded to `size` bytes.
fn png(size: usize) -> Vec<u8> {
    let mut png = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
        0, 0, 64, 0, 0, 0, 48, 8, 6, 0, 0, 0,
    ];
    png.extend((0..size - png.len()).map(|i| (i % 253) as u8));
    png
}

#[tokio::test]
async fn files_go_up_with_a_message_and_come_down_for_members() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let cache = tempfile::tempdir().unwrap();
    let bob_client = babel_sdk::Client::register(&server.url, "bob", PASSWORD, "en-US", None)
        .await
        .unwrap();
    let mut bob = connect(
        &bob_client,
        SessionConfig {
            media_cache: Some(cache.path().join("bob.media")),
            ..SessionConfig::default()
        },
    )
    .await;
    let carol = user(&server, "carol", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;

    let picture = png(1_500_000);
    let photo = alice
        .session
        .upload(&channel, "練習.png", picture.clone())
        .await
        .unwrap();
    assert_eq!(photo.content_type, "image/png");
    assert_eq!((photo.width, photo.height), (Some(64), Some(48)));
    let notes = alice
        .session
        .upload(&channel, "notes.txt", b"bring a capo".to_vec())
        .await
        .unwrap();
    assert_eq!(notes.content_type, "application/octet-stream");

    // Files alone: nothing to translate.
    let sent = alice
        .session
        .send_with(&channel, "", None, &[photo.clone(), notes.clone()])
        .await
        .unwrap();
    let id = sent.message.id.clone();
    let seen = bob.settled(&channel, &id).await;
    assert_eq!(seen.display.shown, Shown::Original);
    let names = seen
        .message
        .attachments
        .iter()
        .map(|file| file.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["練習.png", "notes.txt"]);

    // Members download them, once: then they are on the device.
    let file = &seen.message.attachments[0];
    assert_eq!(bob.session.download(file).await.unwrap(), picture);
    for _ in 0..100 {
        if cache.path().join("bob.media").join(&file.id).exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(cache.path().join("bob.media").join(&file.id).exists());
    assert_eq!(
        bob.session
            .download(&seen.message.attachments[1])
            .await
            .unwrap(),
        b"bring a capo".as_slice()
    );
    // Nobody else.
    assert!(matches!(
        carol.session.download(file).await,
        Err(Error::Api(ApiError::NotFound))
    ));

    // An upload goes with one message only.
    assert!(matches!(
        alice
            .session
            .send_with(&channel, "again", None, &[notes])
            .await,
        Err(Error::Api(ApiError::InvalidRequest))
    ));
    // Too large is refused before it is sent.
    assert!(matches!(
        alice
            .session
            .upload(&channel, "big.bin", vec![0; MAX_UPLOAD_BYTES as usize + 1])
            .await,
        Err(Error::Api(ApiError::LimitExceeded))
    ));

    // Signing out forgets downloaded files.
    bob.session.forget_cache();
    assert!(!cache.path().join("bob.media").exists());
}
