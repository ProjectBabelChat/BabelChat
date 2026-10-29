//! What makes chat usable every day, through the SDK against a whole
//! in-process server: reactions, seeing others write, messages of several
//! lines, threads, and renaming and deleting guilds and channels.

mod common;

use babel_api_translation_cell::types::Plan;
use babel_sdk::{ApiError, Error, GuildChange, Update};
use common::{guild, server, user};

#[tokio::test]
async fn reactions_reach_everyone_with_their_own_marked() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;
    let sent = alice.session.send(&channel, "明日は晴れ").await.unwrap();
    let id = sent.message.id.clone();
    bob.settled(&channel, &id).await;

    bob.session.react(&channel, &id, "👍").await.unwrap();
    // The same emoji, unqualified: still one reaction.
    alice.session.react(&channel, &id, "❤").await.unwrap();
    bob.session.react(&channel, &id, "❤️").await.unwrap();

    // Emoji, count, and whether this reader is among them.
    let reactions = |session: &babel_sdk::Session| {
        session.message(&channel, &id).map(|message| {
            message
                .message
                .reactions
                .iter()
                .map(|reaction| (reaction.emoji.clone(), reaction.count, reaction.me))
                .collect::<Vec<_>>()
        })
    };
    let expected = |thumb_mine: bool| vec![("👍".into(), 1, thumb_mine), ("❤️".into(), 2, true)];
    alice
        .until(|session| (reactions(session)? == expected(false)).then_some(()))
        .await;
    bob.until(|session| (reactions(session)? == expected(true)).then_some(()))
        .await;

    // Taking one back; reacting is not editing.
    bob.session.unreact(&channel, &id, "👍").await.unwrap();
    let message = alice
        .until(|session| {
            session
                .message(&channel, &id)
                .filter(|message| message.message.reactions.len() == 1)
        })
        .await;
    assert_eq!(message.message.rev, 1);
    assert_eq!(message.message.edited_at, None);

    assert!(matches!(
        alice.session.react(&channel, &id, "smile").await,
        Err(Error::Api(ApiError::InvalidRequest))
    ));
}

#[tokio::test]
async fn others_see_someone_write_until_the_message_arrives() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;

    alice.session.typing(&channel);
    let writing = bob
        .until(|session| {
            let members = session.typing_members(&channel);
            (!members.is_empty()).then_some(members)
        })
        .await;
    assert_eq!(writing[0].name, "alice");
    // Not to herself.
    assert!(alice.session.typing_members(&channel).is_empty());

    // Several lines go as they are.
    let sent = alice
        .session
        .send(&channel, "一行目\n二行目")
        .await
        .unwrap();
    assert_eq!(sent.message.text, "一行目\n二行目");
    bob.until(|session| session.typing_members(&channel).is_empty().then_some(()))
        .await;
    let read = bob.settled(&channel, &sent.message.id).await;
    assert_eq!(read.message.text, "一行目\n二行目");
}

#[tokio::test]
async fn threads_channels_and_guilds_are_renamed_and_deleted() {
    let server = server(Plan::Paid3).await;
    let mut alice = user(&server, "alice", "ja-JP").await;
    let mut bob = user(&server, "bob", "en-US").await;
    let channel = guild(&mut alice, &mut [&mut bob], &["ja", "en"], &["ja", "en"]).await;
    let guild_id = alice.session.guilds()[0].guild.id.clone();

    // Bob starts a thread; it is his to rename, not the channel.
    let starter = bob.session.send(&channel, "Setlist?").await.unwrap();
    let thread = bob
        .session
        .start_thread(&channel, &starter.message.id, "setlist")
        .await
        .unwrap();
    assert!(bob.session.may_manage(&thread.id));
    assert!(!bob.session.may_manage(&channel));
    assert!(matches!(
        bob.session.update_channel(&channel, "lounge", None).await,
        Err(Error::Api(ApiError::NotFound))
    ));
    bob.session
        .update_channel(&thread.id, "covers", None)
        .await
        .unwrap();
    alice
        .until(|session| {
            session
                .channel(&thread.id)
                .filter(|thread| thread.name == "covers")
        })
        .await;

    // The owner renames the guild; Bob hears it.
    alice
        .session
        .update_guild(&guild_id, "Jazz Club", Some("Fridays"))
        .await
        .unwrap();
    bob.next(|update| match update {
        Update::Guild {
            change: GuildChange::Updated,
            ..
        } => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(bob.session.guilds()[0].guild.name, "Jazz Club");

    // Deleting the channel takes its thread along.
    alice.session.delete_channel(&channel).await.unwrap();
    bob.until(|session| {
        (session.channel(&channel).is_none() && session.channel(&thread.id).is_none()).then_some(())
    })
    .await;
    assert!(bob.session.messages(&channel).is_empty());

    // Only the owner deletes the guild; then it is gone for everyone.
    assert!(bob.session.delete_guild(&guild_id).await.is_err());
    alice.session.delete_guild(&guild_id).await.unwrap();
    bob.next(|update| match update {
        Update::Guild {
            change: GuildChange::Deleted,
            ..
        } => Some(()),
        _ => None,
    })
    .await;
    assert!(bob.session.guilds().is_empty());
    assert!(alice.session.guilds().is_empty());
}
