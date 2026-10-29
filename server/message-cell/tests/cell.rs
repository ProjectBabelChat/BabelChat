//! Message Cell behavior with a real Translation Cell behind its outbox.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use babel_api_directory::types::{
    Directory, DirectoryError, GuildRequest, MembershipBatch, Placement, PlacementBatch,
};
use babel_api_message_cell::types::{
    ChannelKind, ChannelNotify, ChannelUpdate, GuildEventKind, GuildUpdate, MessageCell as _,
    MessageCellError, MessageEdit, NewChannel, NewGuild, NewInvite, NewMessage,
    NotificationSettings, NotifyLevel, Permission, ReadState,
};
use babel_api_translation_cell::types::{MessageRef, TranslationCell as _, TranslationQuery};
use babel_engine::mock::MockConfig;
use babel_message_cell::MessageCell;
use babel_store::{KvStore, MemStore};
use babel_translation_cell::{TranslationCell, TranslationCellConfig};
use parking_lot::Mutex;

#[derive(Default)]
struct FakeDirectory {
    memberships: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Directory for FakeDirectory {
    async fn create_guild(&self, _: GuildRequest) -> Result<Placement, DirectoryError> {
        Err(DirectoryError::Unavailable)
    }
    async fn get_placement(&self, _: String) -> Result<Placement, DirectoryError> {
        Err(DirectoryError::NotFound)
    }
    async fn watch_placements(&self, after: i64, _: i64) -> Result<PlacementBatch, DirectoryError> {
        Ok(PlacementBatch {
            placements: Vec::new(),
            version: after,
        })
    }
    async fn add_membership(
        &self,
        user_id: String,
        guild_id: String,
    ) -> Result<(), DirectoryError> {
        self.memberships.lock().push((user_id, guild_id));
        Ok(())
    }
    async fn remove_membership(
        &self,
        user_id: String,
        guild_id: String,
    ) -> Result<(), DirectoryError> {
        self.memberships
            .lock()
            .retain(|known| *known != (user_id.clone(), guild_id.clone()));
        Ok(())
    }
    async fn list_memberships(&self, _: String) -> Result<Vec<String>, DirectoryError> {
        Ok(Vec::new())
    }
    async fn watch_memberships(
        &self,
        after: i64,
        _: i64,
    ) -> Result<MembershipBatch, DirectoryError> {
        Ok(MembershipBatch {
            changes: Vec::new(),
            version: after,
            trimmed: 0,
        })
    }
    async fn move_guild(&self, _: String, _: String) -> Result<Placement, DirectoryError> {
        Err(DirectoryError::Unavailable)
    }
}

struct Fixture {
    cell: MessageCell,
    translation: TranslationCell,
    directory: Arc<FakeDirectory>,
}

fn fixture() -> Fixture {
    let engine = Arc::new(babel_engine::Engine::mock(MockConfig {
        latency: Duration::ZERO,
        ..MockConfig::default()
    }));
    let translation = TranslationCell::open(
        Arc::new(MemStore::new()),
        engine,
        TranslationCellConfig::default(),
    )
    .unwrap();

    let mut translation_cells: HashMap<
        String,
        Arc<dyn babel_api_translation_cell::types::TranslationCell>,
    > = HashMap::new();
    translation_cells.insert("tc-0".into(), Arc::new(translation.clone()));
    let store: Arc<dyn KvStore> = Arc::new(MemStore::new());
    let cell = MessageCell::open(store, translation_cells).unwrap();
    let directory = Arc::new(FakeDirectory::default());
    cell.connect_directory(directory.clone());

    Fixture {
        cell,
        translation,
        directory,
    }
}

async fn guild(cell: &MessageCell) -> String {
    cell.create_guild(NewGuild {
        guild_id: "g1".into(),
        translation_cell: "tc-0".into(),
        name: "Rhythm Club".into(),
        description: None,
        owner_id: "owner".into(),
        owner_name: "Owner".into(),
    })
    .await
    .unwrap()
    .id
}

async fn channel(cell: &MessageCell, guild: &str) -> String {
    cell.create_channel(NewChannel {
        guild_id: guild.into(),
        user_id: "owner".into(),
        kind: ChannelKind::Text,
        name: "general".into(),
        topic: None,
        parent_id: None,
        starter_message_id: None,
    })
    .await
    .unwrap()
    .id
}

async fn invite(cell: &MessageCell, guild: &str, max_uses: Option<i64>) -> String {
    cell.create_invite(NewInvite {
        guild_id: guild.into(),
        user_id: "owner".into(),
        expires_at: None,
        max_uses,
    })
    .await
    .unwrap()
    .secret
}

async fn send(cell: &MessageCell, guild: &str, channel: &str, user: &str, text: &str) -> String {
    cell.send_message(NewMessage {
        guild_id: guild.into(),
        channel_id: channel.into(),
        user_id: user.into(),
        author_locale: Some("en-US".into()),
        text: text.into(),
        reply_to: None,
    })
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn messages_reach_the_translation_cell_through_the_outbox() {
    let f = fixture();
    let guild = guild(&f.cell).await;
    let channel = channel(&f.cell, &guild).await;
    let id = send(
        &f.cell,
        &guild,
        &channel,
        "owner",
        "Hello everyone, welcome to the club!",
    )
    .await;

    let reference = MessageRef {
        space: guild.clone(),
        conversation: channel,
        id,
    };
    for _ in 0..200 {
        let found = f
            .translation
            .get_translations(TranslationQuery {
                messages: vec![reference.clone()],
                langs: Vec::new(),
            })
            .await
            .unwrap();
        if let Some(translation) = found.first() {
            assert_eq!(translation.source_lang, "en-US");
            assert_eq!(translation.rev, "1");
            assert!(
                f.directory
                    .memberships
                    .lock()
                    .contains(&("owner".into(), guild))
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the message never reached the translation cell");
}

#[tokio::test]
async fn permissions_follow_roles() {
    let f = fixture();
    let guild = guild(&f.cell).await;
    let channel = channel(&f.cell, &guild).await;

    assert_eq!(
        f.cell
            .get_guild(guild.clone(), "stranger".into())
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
    assert_eq!(
        f.cell
            .get_guild("nope".into(), "owner".into())
            .await
            .unwrap_err(),
        MessageCellError::NotFound
    );

    let invite = invite(&f.cell, &guild, None).await;
    f.cell
        .join_guild(guild.clone(), invite, "member".into(), "Member".into())
        .await
        .unwrap();
    assert_eq!(
        f.cell
            .check_permission(guild.clone(), "member".into(), Permission::ManageGuild)
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );

    // Members may start threads from a message, but not edit others' messages.
    let starter = send(
        &f.cell,
        &guild,
        &channel,
        "owner",
        "Who is up for a match tonight?",
    )
    .await;
    let thread = f
        .cell
        .create_channel(NewChannel {
            guild_id: guild.clone(),
            user_id: "member".into(),
            kind: ChannelKind::Thread,
            name: "match".into(),
            topic: None,
            parent_id: Some(channel.clone()),
            starter_message_id: Some(starter.clone()),
        })
        .await
        .unwrap();
    assert_eq!(thread.kind, ChannelKind::Thread);
    assert_eq!(
        f.cell
            .edit_message(MessageEdit {
                guild_id: guild,
                channel_id: channel,
                message_id: starter,
                user_id: "member".into(),
                text: "hijacked".into(),
            })
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
}

#[tokio::test]
async fn history_pages_newest_first_and_events_stream() {
    let f = fixture();
    let guild = guild(&f.cell).await;
    let channel = channel(&f.cell, &guild).await;
    let mut ids = Vec::new();
    for i in 0..5 {
        ids.push(send(&f.cell, &guild, &channel, "owner", &format!("message {i}")).await);
    }

    let page = f
        .cell
        .get_messages(guild.clone(), channel.clone(), "owner".into(), None, 2)
        .await
        .unwrap();
    assert_eq!(
        page.iter().map(|m| &m.id).collect::<Vec<_>>(),
        [&ids[4], &ids[3]]
    );
    let older = f
        .cell
        .get_messages(
            guild.clone(),
            channel,
            "owner".into(),
            Some(ids[3].clone()),
            10,
        )
        .await
        .unwrap();
    assert_eq!(older.len(), 3);
    assert_eq!(older[0].id, ids[2]);

    let batch = f.cell.poll_events(guild.clone(), 0, 0).await.unwrap();
    let kinds = batch.events.iter().map(|e| e.kind).collect::<Vec<_>>();
    assert_eq!(
        kinds[..3],
        [
            GuildEventKind::GuildCreated,
            GuildEventKind::MemberJoined,
            GuildEventKind::ChannelCreated,
        ]
    );
    assert_eq!(batch.next, 8);

    // A long poll wakes up on the next event.
    let cell = f.cell.clone();
    let waiter = tokio::spawn(async move { cell.poll_events(guild, 8, 5_000).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(20)).await;
    send(
        &f.cell,
        "g1",
        &batch.events[2].channel.as_ref().unwrap().id,
        "owner",
        "late",
    )
    .await;
    let late = waiter.await.unwrap();
    assert_eq!(late.events[0].kind, GuildEventKind::MessageCreated);
}

#[tokio::test]
async fn invites_admit_until_they_run_out() {
    let f = fixture();
    let guild = guild(&f.cell).await;
    let once = invite(&f.cell, &guild, Some(1)).await;

    let join = |invite: &str, user: &str| {
        f.cell
            .join_guild(guild.clone(), invite.into(), user.into(), user.into())
    };
    join(&once, "first").await.unwrap();
    // Used up, wrong and expired invites all look like a missing guild.
    assert_eq!(
        join(&once, "second").await.unwrap_err(),
        MessageCellError::NotFound
    );
    assert_eq!(
        join("0123", "second").await.unwrap_err(),
        MessageCellError::NotFound
    );
    let expired = f
        .cell
        .create_invite(NewInvite {
            guild_id: guild.clone(),
            user_id: "owner".into(),
            expires_at: Some(1),
            max_uses: None,
        })
        .await
        .unwrap()
        .secret;
    assert_eq!(
        join(&expired, "second").await.unwrap_err(),
        MessageCellError::NotFound
    );
    // Members joining again need no valid invite.
    join("0123", "first").await.unwrap();

    // Strangers cannot invite; members can, and may revoke their own.
    assert_eq!(
        f.cell
            .create_invite(NewInvite {
                guild_id: guild.clone(),
                user_id: "stranger".into(),
                expires_at: None,
                max_uses: None,
            })
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
    let theirs = f
        .cell
        .create_invite(NewInvite {
            guild_id: guild.clone(),
            user_id: "first".into(),
            expires_at: None,
            max_uses: None,
        })
        .await
        .unwrap()
        .secret;
    let mine = invite(&f.cell, &guild, None).await;
    assert_eq!(
        f.cell
            .revoke_invite(guild.clone(), mine.clone(), "first".into())
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
    f.cell
        .revoke_invite(guild.clone(), theirs.clone(), "first".into())
        .await
        .unwrap();
    assert_eq!(
        join(&theirs, "second").await.unwrap_err(),
        MessageCellError::NotFound
    );
}

/// The read state of `user` in `channel`, as their snapshot shows it.
async fn read_state(cell: &MessageCell, guild: &str, channel: &str, user: &str) -> ReadState {
    cell.get_guild(guild.into(), user.into())
        .await
        .unwrap()
        .read_states
        .into_iter()
        .find(|state| state.channel_id == channel)
        .unwrap()
}

async fn mentions(cell: &MessageCell, guild: &str, channel: &str, user: &str) -> i64 {
    read_state(cell, guild, channel, user).await.mention_count
}

/// A guild with a channel, its owner and members "alice" and "bob".
async fn club(cell: &MessageCell) -> (String, String) {
    let guild = guild(cell).await;
    let channel = channel(cell, &guild).await;
    let invite = invite(cell, &guild, None).await;
    for user in ["alice", "bob"] {
        cell.join_guild(guild.clone(), invite.clone(), user.into(), user.into())
            .await
            .unwrap();
    }
    (guild, channel)
}

#[tokio::test]
async fn mentions_and_replies_count_until_read() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let cell = &f.cell;

    // Joining reads nothing; everything after it is unread.
    let joined = read_state(cell, &guild, &channel, "bob").await;
    assert_eq!(joined.mention_count, 0);

    let hello = cell
        .send_message(NewMessage {
            guild_id: guild.clone(),
            channel_id: channel.clone(),
            user_id: "alice".into(),
            author_locale: None,
            text: "hi @BOB, and @owner! mail@alice @bobby".into(),
            reply_to: None,
        })
        .await
        .unwrap();
    assert_eq!(hello.mentions.unwrap(), ["bob", "owner"]);
    assert!(hello.id.parse::<u64>().unwrap() > joined.last_read_id.parse::<u64>().unwrap());
    assert_eq!(mentions(cell, &guild, &channel, "bob").await, 1);
    assert_eq!(mentions(cell, &guild, &channel, "owner").await, 1);
    assert_eq!(mentions(cell, &guild, &channel, "alice").await, 0);

    // A reply notifies the author of the message it answers.
    let reply = cell
        .send_message(NewMessage {
            guild_id: guild.clone(),
            channel_id: channel.clone(),
            user_id: "owner".into(),
            author_locale: None,
            text: "welcome!".into(),
            reply_to: Some(hello.id.clone()),
        })
        .await
        .unwrap();
    assert_eq!(reply.mentions.unwrap(), ["alice"]);
    // Writing reads the channel.
    assert_eq!(mentions(cell, &guild, &channel, "owner").await, 0);
    assert_eq!(
        read_state(cell, &guild, &channel, "owner")
            .await
            .last_read_id,
        reply.id
    );

    // Edits and deletions take mentions back from those who had not read.
    let ping = send(cell, &guild, &channel, "alice", "@bob ping").await;
    assert_eq!(mentions(cell, &guild, &channel, "bob").await, 2);
    cell.edit_message(MessageEdit {
        guild_id: guild.clone(),
        channel_id: channel.clone(),
        message_id: ping.clone(),
        user_id: "alice".into(),
        text: "ping".into(),
    })
    .await
    .unwrap();
    assert_eq!(mentions(cell, &guild, &channel, "bob").await, 1);
    let again = send(cell, &guild, &channel, "alice", "@bob ping again").await;
    assert_eq!(mentions(cell, &guild, &channel, "bob").await, 2);
    cell.delete_message(guild.clone(), channel.clone(), again, "alice".into())
        .await
        .unwrap();
    assert_eq!(mentions(cell, &guild, &channel, "bob").await, 1);

    // Reading up to a message recounts what comes after it; reading never
    // goes backwards and never past the newest message.
    let later = send(cell, &guild, &channel, "alice", "@bob later").await;
    let state = cell
        .ack(
            guild.clone(),
            channel.clone(),
            "bob".into(),
            hello.id.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        (state.last_read_id.as_str(), state.mention_count),
        (hello.id.as_str(), 1)
    );
    let state = cell
        .ack(
            guild.clone(),
            channel.clone(),
            "bob".into(),
            joined.last_read_id,
        )
        .await
        .unwrap();
    assert_eq!(state.last_read_id, hello.id);
    let state = cell
        .ack(
            guild.clone(),
            channel.clone(),
            "bob".into(),
            u64::MAX.to_string(),
        )
        .await
        .unwrap();
    assert_eq!((state.last_read_id, state.mention_count), (later, 0));
    assert_eq!(
        cell.ack(guild.clone(), channel.clone(), "stranger".into(), ping)
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
}

#[tokio::test]
async fn members_leave_and_get_removed() {
    let f = fixture();
    let (guild, _) = club(&f.cell).await;
    let cell = &f.cell;
    let invite = invite(cell, &guild, None).await;

    // The owner cannot leave; members can, and lose access.
    assert_eq!(
        cell.leave_guild(guild.clone(), "owner".into())
            .await
            .unwrap_err(),
        MessageCellError::InvalidRequest
    );
    cell.leave_guild(guild.clone(), "alice".into())
        .await
        .unwrap();
    assert_eq!(
        cell.get_guild(guild.clone(), "alice".into())
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );

    // Only the owner removes members; a ban keeps them out until lifted.
    assert_eq!(
        cell.remove_member(guild.clone(), "bob".into(), "owner".into(), false)
            .await
            .unwrap_err(),
        MessageCellError::Forbidden
    );
    cell.remove_member(guild.clone(), "owner".into(), "bob".into(), true)
        .await
        .unwrap();
    let bans = cell.get_bans(guild.clone(), "owner".into()).await.unwrap();
    assert_eq!(bans.len(), 1);
    assert_eq!(
        (bans[0].name.as_str(), bans[0].banned_by.as_str()),
        ("bob", "owner")
    );
    assert_eq!(
        cell.join_guild(guild.clone(), invite.clone(), "bob".into(), "bob".into())
            .await
            .unwrap_err(),
        MessageCellError::NotFound
    );
    cell.unban_member(guild.clone(), "owner".into(), "bob".into())
        .await
        .unwrap();
    cell.join_guild(guild.clone(), invite.clone(), "bob".into(), "bob".into())
        .await
        .unwrap();
    // Leaving and kicking are different events; both reach the Directory.
    cell.join_guild(guild.clone(), invite, "alice".into(), "alice".into())
        .await
        .unwrap();
    cell.remove_member(guild.clone(), "owner".into(), "alice".into(), false)
        .await
        .unwrap();

    let kinds = cell
        .poll_events(guild.clone(), 0, 0)
        .await
        .unwrap()
        .events
        .into_iter()
        .filter(|event| event.member.as_ref().is_some_and(|m| m.user_id != "owner"))
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    use GuildEventKind::*;
    assert_eq!(
        kinds,
        [
            MemberJoined,
            MemberJoined,
            MemberLeft,
            MemberRemoved,
            MemberJoined,
            MemberJoined,
            MemberRemoved
        ]
    );
    for _ in 0..200 {
        let memberships = f.directory.memberships.lock().clone();
        if memberships.len() == 2 {
            assert!(memberships.contains(&("bob".into(), guild.clone())));
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the Directory never learned who left");
}

#[tokio::test]
async fn invites_are_listed_to_their_creators() {
    let f = fixture();
    let (guild, _) = club(&f.cell).await;
    let cell = &f.cell;
    let new_invite = |user: &str, expires_at: Option<i64>| {
        cell.create_invite(NewInvite {
            guild_id: guild.clone(),
            user_id: user.into(),
            expires_at,
            max_uses: None,
        })
    };
    // `club` made one already.
    new_invite("owner", None).await.unwrap();
    new_invite("owner", Some(1)).await.unwrap();
    let alices = new_invite("alice", None).await.unwrap();

    let all = cell
        .get_invites(guild.clone(), "owner".into())
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    let own = cell
        .get_invites(guild.clone(), "alice".into())
        .await
        .unwrap();
    assert_eq!(own, [alices]);
    assert!(cell.get_invites(guild, "stranger".into()).await.is_err());
}

#[tokio::test]
async fn notification_settings_belong_to_each_member() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let cell = &f.cell;
    let snapshot = cell.get_guild(guild.clone(), "bob".into()).await.unwrap();
    assert_eq!(snapshot.notifications.level, NotifyLevel::All);

    let saved = cell
        .put_notification_settings(
            NotificationSettings {
                guild_id: guild.clone(),
                level: NotifyLevel::Mentions,
                channels: vec![
                    ChannelNotify {
                        channel_id: channel.clone(),
                        level: NotifyLevel::All,
                    },
                    ChannelNotify {
                        channel_id: "unknown".into(),
                        level: NotifyLevel::None,
                    },
                    ChannelNotify {
                        channel_id: channel.clone(),
                        level: NotifyLevel::None,
                    },
                ],
            },
            "bob".into(),
        )
        .await
        .unwrap();
    assert_eq!(saved.channels.len(), 1);
    assert_eq!(saved.channels[0].level, NotifyLevel::None);
    let bobs = cell.get_guild(guild.clone(), "bob".into()).await.unwrap();
    assert_eq!(bobs.notifications, saved);
    let alices = cell.get_guild(guild.clone(), "alice".into()).await.unwrap();
    assert_eq!(alices.notifications.level, NotifyLevel::All);
}

/// Waits until the Translation Cell has message `id`: the outbox delivered
/// everything before it.
async fn delivered(f: &Fixture, guild: &str, channel: &str, id: &str) {
    let reference = MessageRef {
        space: guild.into(),
        conversation: channel.into(),
        id: id.into(),
    };
    for _ in 0..200 {
        let found = f
            .translation
            .get_translations(TranslationQuery {
                messages: vec![reference.clone()],
                langs: Vec::new(),
            })
            .await
            .unwrap();
        if !found.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("the outbox never delivered {id}");
}

#[tokio::test]
async fn logs_are_trimmed_once_delivered() {
    let f = fixture();
    let guild = guild(&f.cell).await;
    let channel = channel(&f.cell, &guild).await;
    let mut last = String::new();
    for n in 0..5 {
        last = send(&f.cell, &guild, &channel, "owner", &format!("message {n}")).await;
    }
    delivered(&f, &guild, &channel, &last).await;
    // The outbox deletes an entry right after delivering it.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Nothing is old enough yet with an hour's retention...
    assert_eq!(
        f.cell.trim_logs(Duration::from_secs(3600)).await.unwrap(),
        0
    );
    // ...and everything is without one.
    let head = f.cell.poll_events(guild.clone(), 0, 0).await.unwrap().head;
    assert_eq!(
        f.cell.trim_logs(Duration::ZERO).await.unwrap(),
        head as usize
    );

    // A follower from the start learns it missed events; one at the head
    // carries on.
    let behind = f.cell.poll_events(guild.clone(), 0, 0).await.unwrap();
    assert!(behind.events.is_empty());
    assert_eq!(behind.trimmed, head);
    send(&f.cell, &guild, &channel, "owner", "still here").await;
    let batch = f.cell.poll_events(guild.clone(), head, 0).await.unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].kind, GuildEventKind::MessageCreated);
}

#[tokio::test]
async fn undelivered_entries_are_kept() {
    // The Translation Cell is gone: the outbox holds every entry.
    let mut translation_cells: HashMap<
        String,
        Arc<dyn babel_api_translation_cell::types::TranslationCell>,
    > = HashMap::new();
    let (peer, url) = (
        babel_common::Identity {
            role: babel_common::Role::TranslationCell,
            name: "tc-0".into(),
        },
        "https://127.0.0.1:9",
    );
    translation_cells.insert(
        "tc-0".into(),
        Arc::new(babel_net::remote::RemoteTranslationCell::new(
            peer,
            url,
            reqwest::Client::new(),
        )),
    );
    let cell = MessageCell::open(Arc::new(MemStore::new()), translation_cells).unwrap();
    cell.connect_directory(Arc::new(FakeDirectory::default()));
    let guild = guild(&cell).await;
    let channel = channel(&cell, &guild).await;
    send(&cell, &guild, &channel, "owner", "hello?").await;

    assert_eq!(cell.trim_logs(Duration::ZERO).await.unwrap(), 0);
    let batch = cell.poll_events(guild, 0, 0).await.unwrap();
    assert_eq!(batch.trimmed, 0);
    assert_eq!(batch.events[0].kind, GuildEventKind::GuildCreated);
}

async fn events_after(cell: &MessageCell, guild: &str, after: i64) -> Vec<GuildEventKind> {
    cell.poll_events(guild.into(), after, 0)
        .await
        .unwrap()
        .events
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

async fn head(cell: &MessageCell, guild: &str) -> i64 {
    cell.poll_events(guild.into(), i64::MAX, 0)
        .await
        .unwrap()
        .head
}

#[tokio::test]
async fn reactions_count_each_member_once() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let id = send(&f.cell, &guild, &channel, "owner", "Rehearsal at seven").await;
    let react = |user: &str, emoji: &str, add: bool| {
        let (cell, guild, channel, id) =
            (f.cell.clone(), guild.clone(), channel.clone(), id.clone());
        let (user, emoji) = (user.to_string(), emoji.to_string());
        async move {
            if add {
                cell.add_reaction(guild, channel, id, user, emoji).await
            } else {
                cell.remove_reaction(guild, channel, id, user, emoji).await
            }
        }
    };
    let start = head(&f.cell, &guild).await;

    react("alice", "👍", true).await.unwrap();
    react("alice", "👍", true).await.unwrap();
    // The unqualified heart is the same emoji as the qualified one.
    react("bob", "❤", true).await.unwrap();
    let message = react("alice", "❤️", true).await.unwrap();
    let reactions = message.reactions.unwrap();
    assert_eq!(reactions.len(), 2);
    assert_eq!(reactions[0].emoji, "👍");
    assert_eq!(reactions[0].user_ids, ["alice"]);
    assert_eq!(reactions[1].emoji, "❤️");
    assert_eq!(reactions[1].user_ids, ["bob", "alice"]);
    // Not an edit.
    assert_eq!(message.rev, 1);
    assert_eq!(message.edited_at, None);

    let batch = f.cell.poll_events(guild.clone(), start, 0).await.unwrap();
    let counts = batch
        .events
        .iter()
        .map(|event| {
            let reaction = event.reaction.as_ref().unwrap();
            (reaction.user_id.as_str(), reaction.count)
        })
        .collect::<Vec<_>>();
    assert_eq!(counts, [("alice", 1), ("bob", 1), ("alice", 2)]);

    let message = react("alice", "👍", false).await.unwrap();
    assert_eq!(message.reactions.unwrap().len(), 1);
    assert_eq!(
        events_after(&f.cell, &guild, batch.next).await,
        [GuildEventKind::ReactionRemoved]
    );

    assert_eq!(
        react("alice", "not an emoji", true).await,
        Err(MessageCellError::InvalidRequest)
    );
    assert_eq!(
        react("stranger", "👍", true).await,
        Err(MessageCellError::Forbidden)
    );
    for emoji in emojis::iter().skip(1).take(19) {
        react("alice", emoji.as_str(), true).await.unwrap();
    }
    assert_eq!(
        react("alice", "🎉", true).await,
        Err(MessageCellError::InvalidRequest)
    );
}

#[tokio::test]
async fn typing_only_passes_through_the_log() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let start = head(&f.cell, &guild).await;
    f.cell
        .typing(guild.clone(), channel.clone(), "alice".into())
        .await
        .unwrap();
    let batch = f.cell.poll_events(guild.clone(), start, 0).await.unwrap();
    assert_eq!(batch.events.len(), 1);
    let typing = batch.events[0].typing.as_ref().unwrap();
    assert_eq!(
        (typing.channel_id.as_str(), typing.user_id.as_str()),
        (channel.as_str(), "alice")
    );
    assert_eq!(
        f.cell
            .typing(guild.clone(), channel, "stranger".into())
            .await,
        Err(MessageCellError::Forbidden)
    );
}

async fn thread(cell: &MessageCell, guild: &str, channel: &str, user: &str) -> String {
    let starter = send(cell, guild, channel, user, "Shall we split the setlist?").await;
    cell.create_channel(NewChannel {
        guild_id: guild.into(),
        user_id: user.into(),
        kind: ChannelKind::Thread,
        name: "setlist".into(),
        topic: None,
        parent_id: Some(channel.into()),
        starter_message_id: Some(starter),
    })
    .await
    .unwrap()
    .id
}

fn channel_update(guild: &str, channel: &str, user: &str, name: &str) -> ChannelUpdate {
    ChannelUpdate {
        guild_id: guild.into(),
        channel_id: channel.into(),
        user_id: user.into(),
        name: name.into(),
        topic: Some("What we play".into()),
    }
}

#[tokio::test]
async fn guilds_and_channels_are_renamed() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let rename = |user: &str| GuildUpdate {
        guild_id: guild.clone(),
        user_id: user.into(),
        name: "Jazz Club".into(),
        description: Some("Fridays".into()),
    };
    assert_eq!(
        f.cell.update_guild(rename("alice")).await,
        Err(MessageCellError::Forbidden)
    );
    let start = head(&f.cell, &guild).await;
    let renamed = f.cell.update_guild(rename("owner")).await.unwrap();
    assert_eq!(renamed.name, "Jazz Club");
    assert_eq!(
        f.cell
            .get_guild(guild.clone(), "alice".into())
            .await
            .unwrap()
            .guild
            .description
            .as_deref(),
        Some("Fridays")
    );
    assert_eq!(
        events_after(&f.cell, &guild, start).await,
        [GuildEventKind::GuildUpdated]
    );

    // Channels are the owner's; a thread is its creator's too.
    let thread = thread(&f.cell, &guild, &channel, "alice").await;
    assert_eq!(
        f.cell
            .update_channel(channel_update(&guild, &channel, "alice", "lounge"))
            .await,
        Err(MessageCellError::Forbidden)
    );
    assert_eq!(
        f.cell
            .update_channel(channel_update(&guild, &thread, "bob", "covers"))
            .await,
        Err(MessageCellError::Forbidden)
    );
    let renamed = f
        .cell
        .update_channel(channel_update(&guild, &thread, "alice", "covers"))
        .await
        .unwrap();
    assert_eq!(renamed.name, "covers");
    assert_eq!(renamed.created_by.as_deref(), Some("alice"));
    f.cell
        .update_channel(channel_update(&guild, &channel, "owner", "lounge"))
        .await
        .unwrap();
}

#[tokio::test]
async fn deleted_channels_take_their_threads_and_messages() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let thread = thread(&f.cell, &guild, &channel, "alice").await;
    let id = send(&f.cell, &guild, &thread, "bob", "I can sing the second one").await;
    delivered(&f, &guild, &thread, &id).await;

    assert_eq!(
        f.cell
            .delete_channel(guild.clone(), channel.clone(), "alice".into())
            .await,
        Err(MessageCellError::Forbidden)
    );
    let start = head(&f.cell, &guild).await;
    f.cell
        .delete_channel(guild.clone(), channel.clone(), "owner".into())
        .await
        .unwrap();
    let batch = f.cell.poll_events(guild.clone(), start, 0).await.unwrap();
    let deleted = batch
        .events
        .iter()
        .map(|event| (event.kind, event.channel.as_ref().unwrap().id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        deleted,
        [
            (GuildEventKind::ChannelDeleted, thread.clone()),
            (GuildEventKind::ChannelDeleted, channel.clone()),
        ]
    );
    let snapshot = f.cell.get_guild(guild.clone(), "bob".into()).await.unwrap();
    assert!(snapshot.channels.is_empty());
    assert!(snapshot.read_states.is_empty());
    assert_eq!(
        f.cell
            .get_messages(guild.clone(), thread.clone(), "bob".into(), None, 10)
            .await,
        Err(MessageCellError::NotFound)
    );

    // The Translation Cell forgets the conversation.
    let reference = MessageRef {
        space: guild.clone(),
        conversation: thread,
        id,
    };
    for _ in 0..200 {
        let found = f
            .translation
            .get_translations(TranslationQuery {
                messages: vec![reference.clone()],
                langs: Vec::new(),
            })
            .await
            .unwrap();
        if found.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        f.translation
            .get_translations(TranslationQuery {
                messages: vec![reference],
                langs: Vec::new(),
            })
            .await
            .unwrap()
            .is_empty()
    );

    // A deleted category leaves its channels outside it.
    let category = f
        .cell
        .create_channel(NewChannel {
            guild_id: guild.clone(),
            user_id: "owner".into(),
            kind: ChannelKind::Category,
            name: "music".into(),
            topic: None,
            parent_id: None,
            starter_message_id: None,
        })
        .await
        .unwrap()
        .id;
    let inside = f
        .cell
        .create_channel(NewChannel {
            guild_id: guild.clone(),
            user_id: "owner".into(),
            kind: ChannelKind::Text,
            name: "jams".into(),
            topic: None,
            parent_id: Some(category.clone()),
            starter_message_id: None,
        })
        .await
        .unwrap()
        .id;
    let start = head(&f.cell, &guild).await;
    f.cell
        .delete_channel(guild.clone(), category, "owner".into())
        .await
        .unwrap();
    assert_eq!(
        events_after(&f.cell, &guild, start).await,
        [
            GuildEventKind::ChannelUpdated,
            GuildEventKind::ChannelDeleted
        ]
    );
    let channels = f
        .cell
        .get_guild(guild.clone(), "bob".into())
        .await
        .unwrap()
        .channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].id, inside);
    assert_eq!(channels[0].parent_id, None);
}

#[tokio::test]
async fn deleted_guilds_are_read_to_the_end() {
    let f = fixture();
    let (guild, channel) = club(&f.cell).await;
    let id = send(&f.cell, &guild, &channel, "alice", "See you Friday").await;
    delivered(&f, &guild, &channel, &id).await;

    assert_eq!(
        f.cell.delete_guild(guild.clone(), "alice".into()).await,
        Err(MessageCellError::Forbidden)
    );
    let start = head(&f.cell, &guild).await;
    f.cell
        .delete_guild(guild.clone(), "owner".into())
        .await
        .unwrap();

    assert_eq!(
        f.cell.get_guild(guild.clone(), "owner".into()).await,
        Err(MessageCellError::NotFound)
    );
    assert_eq!(
        f.cell
            .send_message(NewMessage {
                guild_id: guild.clone(),
                channel_id: channel.clone(),
                user_id: "owner".into(),
                author_locale: None,
                text: "Anyone?".into(),
                reply_to: None,
            })
            .await,
        Err(MessageCellError::NotFound)
    );
    // Followers read the deletion, then every member's removal.
    let batch = f.cell.poll_events(guild.clone(), start, 0).await.unwrap();
    let kinds = batch
        .events
        .iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            GuildEventKind::GuildDeleted,
            GuildEventKind::MemberRemoved,
            GuildEventKind::MemberRemoved,
            GuildEventKind::MemberRemoved,
        ]
    );
    assert_eq!(
        f.cell.poll_events(guild.clone(), batch.next, 0).await,
        Err(MessageCellError::NotFound)
    );

    // The Directory and the Translation Cell forget it.
    for _ in 0..200 {
        if f.directory.memberships.lock().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(f.directory.memberships.lock().is_empty());
    let reference = MessageRef {
        space: guild,
        conversation: channel,
        id,
    };
    assert!(
        f.translation
            .get_translations(TranslationQuery {
                messages: vec![reference],
                langs: Vec::new(),
            })
            .await
            .unwrap()
            .is_empty()
    );
}
