//! Chatting through the SDK against a whole in-process server: what an app
//! built on the SDK gets.

mod common;

use std::time::Duration;

use babel_api_translation_cell::types::Plan;
use babel_sdk::{MessageChange, Shown, Update};
use common::{JAPANESE, guild, server, user};

#[tokio::test]
async fn paid_guilds_reach_every_reader_in_their_language() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-GB").await;
    let mut carol = user(&server, "carol", "fr-FR").await;
    let channel = guild(
        &mut alice,
        &mut [&mut bob, &mut carol],
        &["ja", "en", "ko"],
        &["ja", "en"],
    )
    .await;

    let sent = alice.session.send(&channel, JAPANESE).await.unwrap();
    assert_eq!(sent.display.shown, Shown::Original);

    // Bob reads the LLM's English.
    let read = bob.settled(&channel, &sent.message.id).await;
    assert_eq!(
        read.display.shown,
        Shown::Server {
            from: "ja-JP".into(),
            to: "en-US".into()
        }
    );
    assert_eq!(read.display.text, format!("[en-US] {JAPANESE}"));

    // Carol is outside the guild's languages: her device goes on from the
    // LLM's English, one step.
    let read = carol.settled(&channel, &sent.message.id).await;
    assert_eq!(
        read.display.shown,
        Shown::Device {
            from: "ja-JP".into(),
            to: "fr-FR".into(),
            via_english: true
        }
    );
    assert_eq!(
        read.display.text,
        format!("[device fr-FR] [en-US] {JAPANESE}")
    );

    // Replies flow back the same way.
    let reply = bob
        .session
        .send(&channel, "Sounds great, count me in!")
        .await
        .unwrap();
    let read = alice.settled(&channel, &reply.message.id).await;
    assert_eq!(read.display.text, "[ja-JP] Sounds great, count me in!");
}

#[tokio::test]
async fn free_guilds_translate_on_device_through_english() {
    let server = server(Plan::Free).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let mut carol = user(&server, "carol", "fr-FR").await;
    let channel = guild(
        &mut alice,
        &mut [&mut bob, &mut carol],
        &["ja", "en", "fr"],
        &[],
    )
    .await;

    let sent = alice.session.send(&channel, JAPANESE).await.unwrap();

    let read = bob.settled(&channel, &sent.message.id).await;
    assert_eq!(
        read.display.shown,
        Shown::Device {
            from: "ja-JP".into(),
            to: "en-US".into(),
            via_english: false
        }
    );
    assert_eq!(read.display.text, format!("[device en-US] {JAPANESE}"));

    // N → English → N, both steps on the device.
    let read = carol.settled(&channel, &sent.message.id).await;
    assert_eq!(
        read.display.text,
        format!("[device fr-FR] [device en] {JAPANESE}")
    );
}

#[tokio::test]
async fn edits_and_deletions_reach_readers() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;

    let sent = alice.session.send(&channel, JAPANESE).await.unwrap();
    let id = sent.message.id.clone();
    bob.settled(&channel, &id).await;

    alice
        .session
        .edit(&channel, &id, "やっぱり明後日にしませんか")
        .await
        .unwrap();
    let read = bob
        .until(|session| {
            session
                .message(&channel, &id)
                .filter(|message| message.message.rev == 2 && message.display.is_translated())
        })
        .await;
    assert_eq!(read.display.text, "[en-US] やっぱり明後日にしませんか");

    alice.session.delete(&channel, &id).await.unwrap();
    bob.until(|session| session.message(&channel, &id).is_none().then_some(()))
        .await;
}

#[tokio::test]
async fn history_pages_back_with_translations() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [], &["ja", "en"], &["ja", "en"]).await;

    let mut ids = Vec::new();
    for text in ["一つ目", "二つ目", "三つ目"] {
        ids.push(alice.session.send(&channel, text).await.unwrap().message.id);
    }
    // Let the translations land before Bob arrives.
    alice
        .until(|session| {
            let messages = session.messages(&channel);
            messages
                .iter()
                .all(|m| m.translation.as_ref().is_some_and(|t| t.status == "done"))
                .then_some(())
        })
        .await;

    let guild_id = alice.session.channel(&channel).unwrap().guild_id;
    let invite = alice
        .session
        .create_invite(&guild_id, Some(0), Some(1))
        .await
        .unwrap();
    bob.session.join(&invite.code).await.unwrap();

    assert_eq!(bob.session.load_history(&channel, 2).await.unwrap(), 2);
    assert!(!bob.session.history_complete(&channel));
    assert_eq!(bob.session.load_history(&channel, 2).await.unwrap(), 1);
    assert!(bob.session.history_complete(&channel));

    let messages = bob.session.messages(&channel);
    let shown = messages
        .iter()
        .map(|message| message.message.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(shown, ids, "oldest first");
    assert_eq!(messages[0].display.text, "[en-US] 一つ目");
}

#[tokio::test]
async fn changing_locale_changes_what_is_shown() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(
        &mut alice,
        &mut [&mut bob],
        &["ja", "en", "ko"],
        &["ja", "en"],
    )
    .await;

    let sent = alice.session.send(&channel, JAPANESE).await.unwrap();
    bob.session.load_history(&channel, 10).await.unwrap();
    bob.settled(&channel, &sent.message.id).await;

    // Korean is declared but not translated by the LLM: the device takes
    // over from English.
    let mut settings = bob.session.settings();
    settings.locale = "ko-KR".into();
    bob.session.set_settings(settings).await.unwrap();

    let view = bob.session.guilds()[0].language.clone();
    assert_eq!(view.effective, "ko-KR");
    assert!(view.in_set && !view.llm && view.client_translation);
    let read = bob.settled(&channel, &sent.message.id).await;
    assert_eq!(
        read.display.text,
        format!("[device ko-KR] [en-US] {JAPANESE}")
    );
}

#[tokio::test]
async fn invites_are_needed_and_updates_announce_members() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let guild = alice.session.create_guild("Private", None).await.unwrap();

    assert!(bob.session.join(&guild.guild.id).await.is_err());
    assert!(bob.session.join("not-an-invite").await.is_err());

    let invite = alice
        .session
        .create_invite(&guild.guild.id, None, Some(1))
        .await
        .unwrap();
    bob.session.join(&invite.code).await.unwrap();

    let bob_id = bob.session.user_id().to_string();
    alice
        .until(|session| {
            session
                .guild(&guild.guild.id)
                .filter(|view| view.members.iter().any(|m| m.user_id == bob_id))
        })
        .await;

    // Bob sees new channels appear.
    alice
        .session
        .create_channel(&guild.guild.id, "random", None)
        .await
        .unwrap();
    bob.until(|session| {
        session
            .guild(&guild.guild.id)
            .filter(|view| view.channels.iter().any(|c| c.name == "random"))
    })
    .await;

    // Updates name what changed.
    let mut created = None;
    let message = alice
        .session
        .create_channel(&guild.guild.id, "general", None)
        .await
        .unwrap();
    let sent = alice.session.send(&message.id, "やあ").await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while created.is_none() {
        let update = tokio::time::timeout_at(deadline, bob.updates.recv())
            .await
            .unwrap()
            .unwrap();
        if let Update::Message {
            message_id,
            change: MessageChange::Created,
            ..
        } = update
        {
            created = Some(message_id);
        }
    }
    assert_eq!(created.unwrap(), sent.message.id);
}

#[tokio::test]
async fn owners_choose_the_model_of_paid_guilds() {
    let paid = server(Plan::Paid3).await;
    let mut alice = user(&paid, "alice", "ja-JP").await;
    let mut bob = user(&paid, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;
    let guild_id = alice.session.channel(&channel).unwrap().guild_id;

    let models = bob.session.models(&guild_id).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "mock");

    // Only the owner chooses, and only among the guild's models.
    assert!(
        bob.session
            .set_model(&guild_id, Some("mock".into()))
            .await
            .is_err()
    );
    assert!(
        alice
            .session
            .set_model(&guild_id, Some("gpt-9".into()))
            .await
            .is_err()
    );
    let policy = alice
        .session
        .set_model(&guild_id, Some("mock".into()))
        .await
        .unwrap();
    assert_eq!(policy.model.as_deref(), Some("mock"));
    assert_eq!(policy.llm_languages, ["ja-JP", "en-US"]);

    // Bob's session hears of it.
    bob.until(|session| {
        session
            .guild(&guild_id)
            .filter(|view| view.policy.model.as_deref() == Some("mock"))
    })
    .await;

    // Free guilds translate on device: nothing to choose.
    let free = server(Plan::Free).await;
    let mut carol = user(&free, "carol", "ja-JP").await;
    let channel = guild(&mut carol, &mut [], &["ja", "en"], &[]).await;
    let guild_id = carol.session.channel(&channel).unwrap().guild_id;
    assert!(carol.session.models(&guild_id).await.unwrap().is_empty());
}
