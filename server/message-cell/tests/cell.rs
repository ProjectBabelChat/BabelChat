//! Message Cell behavior with a real Translation Cell behind its outbox.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use babel_api_directory::types::{
    Directory, DirectoryError, GuildRequest, Placement, PlacementBatch,
};
use babel_api_message_cell::types::{
    ChannelKind, GuildEventKind, MessageCell as _, MessageCellError, MessageEdit, NewChannel,
    NewGuild, NewMessage, Permission,
};
use babel_api_translation_cell::types::{MessageRef, TranslationCell as _, TranslationQuery};
use babel_engine::mock::{MockBackend, MockConfig};
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
    async fn watch_placements(&self, after: i64, _: i64) -> PlacementBatch {
        PlacementBatch {
            placements: Vec::new(),
            version: after,
        }
    }
    async fn add_membership(&self, user_id: String, guild_id: String) {
        self.memberships.lock().push((user_id, guild_id));
    }
    async fn remove_membership(&self, _: String, _: String) {}
    async fn list_memberships(&self, _: String) -> Vec<String> {
        Vec::new()
    }
}

struct Fixture {
    cell: MessageCell,
    translation: TranslationCell,
    directory: Arc<FakeDirectory>,
}

fn fixture() -> Fixture {
    let engine = Arc::new(babel_engine::Engine::new(Arc::new(MockBackend::new(
        MockConfig {
            latency: Duration::ZERO,
            ..MockConfig::default()
        },
    ))));
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

async fn send(cell: &MessageCell, guild: &str, channel: &str, user: &str, text: &str) -> String {
    cell.send_message(NewMessage {
        guild_id: guild.into(),
        channel_id: channel.into(),
        user_id: user.into(),
        author_locale: Some("en-US".into()),
        text: text.into(),
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

    f.cell
        .join_guild(guild.clone(), "member".into(), "Member".into())
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
