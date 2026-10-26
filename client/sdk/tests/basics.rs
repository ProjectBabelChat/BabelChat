//! Chat basics through the SDK: notifications and unread markers, leaving
//! and removing members, the cache, and both event transports.

mod common;

use babel_api_translation_cell::types::Plan;
use babel_sdk::{
    ApiError, Client, Error, GuildChange, NotificationSettings, NotifyLevel, NotifyReason,
    SessionConfig, Transport, Unread, Update,
};
use common::{JAPANESE, PASSWORD, connect, guild, server, sign_in, user};

#[tokio::test]
async fn mentions_notify_in_the_readers_language_until_read() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;
    let guild_id = alice.session.channel(&channel).unwrap().guild_id;
    // Bob also has a phone.
    let mut phone = sign_in(&server, "bob", SessionConfig::default()).await;

    let sent = alice
        .session
        .send(&channel, &format!("@bob {JAPANESE}"))
        .await
        .unwrap();
    assert_eq!(sent.message.mentions, [bob.session.user_id()]);
    let notification = bob.notification().await;
    assert_eq!(notification.reason, NotifyReason::Mention);
    assert_eq!(
        notification.message.display.text,
        format!("[en-US] @bob {JAPANESE}")
    );
    let mentioned = Unread {
        unread: true,
        mentions: 1,
    };
    assert_eq!(bob.session.unread(&channel), mentioned);
    assert_eq!(bob.session.guild_unread(&guild_id), mentioned);
    phone
        .until(|session| (session.unread(&channel) == mentioned).then_some(()))
        .await;

    // Answering on the phone reads the channel everywhere, and notifies
    // Alice of a reply.
    let reply = phone
        .session
        .reply(&channel, &sent.message.id, "Count me in!")
        .await
        .unwrap();
    assert_eq!(reply.message.mentions, [alice.session.user_id()]);
    bob.until(|session| (session.unread(&channel) == Unread::default()).then_some(()))
        .await;
    let notification = alice.notification().await;
    assert_eq!(notification.reason, NotifyReason::Reply);
    assert_eq!(notification.message.display.text, "[ja-JP] Count me in!");

    // Everything notifies by default; "mentions" quiets the rest, on every
    // device.
    alice.session.send(&channel, "誰か来る？").await.unwrap();
    assert_eq!(bob.notification().await.reason, NotifyReason::Message);
    bob.session
        .set_notifications(NotificationSettings {
            guild_id: guild_id.clone(),
            level: NotifyLevel::Mentions,
            channels: Vec::new(),
        })
        .await
        .unwrap();
    phone
        .until(|session| {
            (session.notification_settings(&guild_id)?.level == NotifyLevel::Mentions).then_some(())
        })
        .await;
    alice.session.send(&channel, "静かに").await.unwrap();
    let wanted = alice.session.send(&channel, "@bob 来て").await.unwrap();
    assert_eq!(
        bob.notification().await.message.message.id,
        wanted.message.id
    );

    // Reading on one device clears the other.
    phone
        .until(|session| session.unread(&channel).unread.then_some(()))
        .await;
    bob.session.mark_read(&channel).await.unwrap();
    phone
        .until(|session| (session.unread(&channel) == Unread::default()).then_some(()))
        .await;

    // The channel on screen neither notifies nor stays unread.
    bob.session.focus(Some(&channel));
    let seen = alice.session.send(&channel, "@bob 見てる？").await.unwrap();
    bob.until(|session| session.message(&channel, &seen.message.id))
        .await;
    phone
        .until(|session| session.message(&channel, &seen.message.id))
        .await;
    phone
        .until(|session| (session.unread(&channel) == Unread::default()).then_some(()))
        .await;
    bob.session.focus(None);
    let later = alice.session.send(&channel, "@bob またね").await.unwrap();
    assert_eq!(
        bob.notification().await.message.message.id,
        later.message.id
    );
}

#[tokio::test]
async fn members_leave_and_owners_remove_them() {
    let server = server(Plan::Free).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let mut carol = user(&server, "carol", "fr-FR").await;
    let channel = guild(&mut alice, &mut [&mut bob, &mut carol], &["ja"], &[]).await;
    let guild_id = alice.session.channel(&channel).unwrap().guild_id;
    let bob_id = bob.session.user_id().to_string();
    let carol_id = carol.session.user_id().to_string();

    assert_eq!(
        alice.session.leave(&guild_id).await.unwrap_err(),
        Error::Api(ApiError::InvalidRequest)
    );
    carol.session.leave(&guild_id).await.unwrap();
    assert!(carol.session.guild(&guild_id).is_none());
    alice
        .next(|update| match update {
            Update::Guild {
                change: GuildChange::MemberLeft { user_id },
                ..
            } if user_id == carol_id => Some(()),
            _ => None,
        })
        .await;

    // A ban removes Bob from all his sessions and keeps him out.
    alice
        .session
        .remove_member(&guild_id, &bob_id, true)
        .await
        .unwrap();
    bob.next(|update| match update {
        Update::Guild {
            change: GuildChange::Removed,
            ..
        } => Some(()),
        _ => None,
    })
    .await;
    assert!(bob.session.guild(&guild_id).is_none());
    assert!(bob.session.messages(&channel).is_empty());
    let elsewhere = sign_in(&server, "bob", SessionConfig::default()).await;
    assert!(elsewhere.session.guilds().is_empty());

    let invites = alice.session.invites(&guild_id).await.unwrap();
    assert_eq!(invites.len(), 1);
    assert_eq!(
        bob.session.join(&invites[0].code).await.unwrap_err(),
        Error::Api(ApiError::NotFound)
    );
    let bans = alice.session.bans(&guild_id).await.unwrap();
    assert_eq!(bans[0].user_id, bob_id);
    assert!(bob.session.bans(&guild_id).await.is_err());

    alice.session.unban(&guild_id, &bob_id).await.unwrap();
    bob.session.join(&invites[0].code).await.unwrap();
    assert!(bob.session.guild(&guild_id).is_some());
}

#[tokio::test]
async fn sessions_start_from_their_cache_even_offline() {
    let server = server(Plan::Paid3).await;
    let dir = tempfile::tempdir().unwrap();
    let config = SessionConfig {
        cache: Some(dir.path().join("alice.json")),
        ..SessionConfig::default()
    };
    let client = Client::register(&server.url, "alice", PASSWORD, "ja-JP")
        .await
        .unwrap();
    let mut alice = connect(&client, config.clone()).await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;

    let first = bob.session.send(&channel, "Hello!").await.unwrap();
    alice.session.load_history(&channel, 50).await.unwrap();
    alice.settled(&channel, &first.message.id).await;
    drop(alice);

    // Bob writes while Alice is away. She starts from the cache at once,
    // then catches up.
    let second = bob.session.send(&channel, "Anyone around?").await.unwrap();
    let mut alice = connect(&client, config.clone()).await;
    let cached = alice.session.messages(&channel);
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].display.text, "[ja-JP] Hello!");
    alice
        .until(|session| session.message(&channel, &second.message.id))
        .await;
    assert!(alice.session.unread(&channel).unread);
    drop(alice);

    // Offline, the cache is all there is.
    server.task.abort();
    let _ = server.task.await;
    let client = Client::with_token(client.server(), client.token().clone()).unwrap();
    let mut alice = connect(&client, config).await;
    assert_eq!(alice.session.messages(&channel).len(), 2);
    alice
        .next(|update| matches!(update, Update::Offline { .. }).then_some(()))
        .await;
}

#[tokio::test]
async fn events_arrive_by_websocket_or_long_polling() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let client = Client::register(&server.url, "bob", PASSWORD, "en-US")
        .await
        .unwrap();
    let mut bob = connect(
        &client,
        SessionConfig {
            transport: Transport::LongPoll,
            ..SessionConfig::default()
        },
    )
    .await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;

    let sent = alice.session.send(&channel, JAPANESE).await.unwrap();
    let read = bob.settled(&channel, &sent.message.id).await;
    assert_eq!(read.display.text, format!("[en-US] {JAPANESE}"));
    let reply = bob.session.send(&channel, "Sure!").await.unwrap();
    let read = alice.settled(&channel, &reply.message.id).await;
    assert_eq!(read.display.text, "[ja-JP] Sure!");

    assert!(alice.session.uses_websocket());
    assert!(!bob.session.uses_websocket());
}
