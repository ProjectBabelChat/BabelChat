//! The services split up as in production, inside one test process: every
//! call between them goes over HTTP and mutual TLS, through the `Remote*`
//! clients, and clients talk to two different Edges.

use std::{collections::HashMap, sync::Arc, time::Duration};

use babel_account::Account;
use babel_api_account::types::Account as AccountApi;
use babel_api_directory::types::Directory as DirectoryApi;
use babel_api_message_cell::types::MessageCell as MessageCellApi;
use babel_api_translation_cell::types::{Plan, TranslationCell as TranslationCellApi};
use babel_common::{Identity, Role};
use babel_directory::Directory;
use babel_edge::{Cluster, Edge, EdgeConfig};
use babel_engine::mock::MockConfig;
use babel_message_cell::MessageCell;
use babel_net::{
    Policy,
    pki::Pki,
    policies,
    remote::{RemoteAccount, RemoteDirectory, RemoteMessageCell, RemoteTranslationCell},
    serve::serve,
    tls::Credentials,
};
use babel_sdk::{
    Client, MockDeviceTranslator, NotificationSettings, NotifyLevel, Session, SessionConfig, Shown,
};
use babel_store::{KvStore, MemStore};
use babel_translation_cell::{TranslationCell, TranslationCellConfig};
use tokio::net::TcpListener;

const MESSAGE_CELLS: [&str; 2] = ["message-cell-0", "message-cell-1"];
const TRANSLATION_CELL: &str = "translation-cell-0";

/// A cluster of in-process services that only reach each other over the
/// network.
pub struct SplitCluster {
    /// Public URLs of the Edges.
    pub edges: Vec<String>,
    /// The Directory, as an operator calls it.
    pub admin: Arc<dyn DirectoryApi>,
    _pki: tempfile::TempDir,
}

struct Internal {
    pki: tempfile::TempDir,
    urls: HashMap<String, String>,
}

impl Internal {
    fn credentials(&self, name: &str) -> Credentials {
        let dir = self.pki.path();
        Credentials::load(
            &dir.join("ca.crt"),
            &dir.join(format!("{name}.crt")),
            &dir.join(format!("{name}.key")),
        )
        .unwrap()
    }

    /// `(peer, url, client)` for `caller` calling `peer`.
    fn peer(&self, caller: &str, role: Role, name: &str) -> (Identity, String, reqwest::Client) {
        let peer = Identity {
            role,
            name: name.into(),
        };
        let client = self.credentials(caller).http_client(&peer).unwrap();
        (peer, self.urls[name].clone(), client)
    }

    fn account(&self, caller: &str) -> Arc<dyn AccountApi> {
        let (peer, url, client) = self.peer(caller, Role::Account, "account");
        Arc::new(RemoteAccount::new(peer, url, client))
    }

    fn directory(&self, caller: &str) -> Arc<dyn DirectoryApi> {
        let (peer, url, client) = self.peer(caller, Role::Directory, "directory");
        Arc::new(RemoteDirectory::new(peer, url, client))
    }

    fn message_cells(&self, caller: &str) -> Vec<(String, Arc<dyn MessageCellApi>)> {
        MESSAGE_CELLS
            .iter()
            .map(|name| {
                let (peer, url, client) = self.peer(caller, Role::MessageCell, name);
                (
                    name.to_string(),
                    Arc::new(RemoteMessageCell::new(peer, url, client)) as Arc<dyn MessageCellApi>,
                )
            })
            .collect()
    }

    fn translation_cells(&self, caller: &str) -> HashMap<String, Arc<dyn TranslationCellApi>> {
        let (peer, url, client) = self.peer(caller, Role::TranslationCell, TRANSLATION_CELL);
        HashMap::from([(
            TRANSLATION_CELL.to_string(),
            Arc::new(RemoteTranslationCell::new(peer, url, client)) as Arc<dyn TranslationCellApi>,
        )])
    }

    fn serve(&self, name: &str, listener: TcpListener, router: axum::Router, policy: Policy) {
        let credentials = self.credentials(name);
        tokio::spawn(async move { serve(router, listener, &credentials, policy).await });
    }
}

fn store() -> Arc<dyn KvStore> {
    Arc::new(MemStore::new())
}

pub async fn start(edges: usize) -> SplitCluster {
    let pki = tempfile::tempdir().unwrap();
    let authority = Pki::new(pki.path());
    authority.init().unwrap();

    let mut services = vec![
        (Role::Account, "account".to_string()),
        (Role::Directory, "directory".to_string()),
        (Role::TranslationCell, TRANSLATION_CELL.to_string()),
    ];
    services.extend(
        MESSAGE_CELLS
            .iter()
            .map(|name| (Role::MessageCell, name.to_string())),
    );
    let mut listeners = HashMap::new();
    let mut urls = HashMap::new();
    for (role, name) in &services {
        authority
            .issue(&Identity {
                role: *role,
                name: name.clone(),
            })
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.insert(
            name.clone(),
            format!("https://{}", listener.local_addr().unwrap()),
        );
        listeners.insert(name.clone(), listener);
    }
    let edge_names = (0..edges).map(|n| format!("edge-{n}")).collect::<Vec<_>>();
    for name in &edge_names {
        authority
            .issue(&Identity {
                role: Role::Edge,
                name: name.clone(),
            })
            .unwrap();
    }
    let internal = Internal { pki, urls };
    let mut listener = |name: &str| listeners.remove(name).unwrap();

    let account = Account::open(store(), None).unwrap();
    let token_key = account.token_public_key().to_vec();
    internal.serve(
        "account",
        listener("account"),
        babel_api_account::router(account),
        policies::account(),
    );

    let engine = Arc::new(babel_engine::Engine::mock(MockConfig {
        latency: Duration::from_millis(20),
        ..MockConfig::default()
    }));
    let translation = TranslationCell::open(
        store(),
        engine,
        TranslationCellConfig {
            default_plan: Plan::Paid3,
            ..TranslationCellConfig::default()
        },
    )
    .unwrap();
    internal.serve(
        TRANSLATION_CELL,
        listener(TRANSLATION_CELL),
        babel_api_translation_cell::router(translation),
        policies::translation_cell(),
    );

    for name in MESSAGE_CELLS {
        let cell = MessageCell::open(store(), internal.translation_cells(name)).unwrap();
        cell.connect_directory(internal.directory(name));
        internal.serve(
            name,
            listener(name),
            babel_api_message_cell::router(cell),
            policies::message_cell(),
        );
    }

    let directory = Directory::open(
        store(),
        internal.message_cells("directory"),
        vec![TRANSLATION_CELL.to_string()],
    )
    .unwrap();
    internal.serve(
        "directory",
        listener("directory"),
        babel_api_directory::router(directory),
        policies::directory(),
    );

    let mut public = Vec::new();
    for name in &edge_names {
        let edge = Edge::start(
            Cluster {
                account: internal.account(name),
                directory: internal.directory(name),
                message_cells: internal.message_cells(name).into_iter().collect(),
                translation_cells: internal.translation_cells(name),
            },
            EdgeConfig {
                token_keys: vec![jsonwebtoken::DecodingKey::from_ed_der(&token_key)],
                writes_per_second: 100.0,
                write_burst: 100.0,
            },
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        public.push(format!("http://{}", listener.local_addr().unwrap()));
        let router = babel_api_edge::router(edge.clone()).merge(edge.websocket_router());
        tokio::spawn(async move { axum::serve(listener, router).await });
    }

    authority
        .issue(&Identity {
            role: Role::Admin,
            name: "ops".into(),
        })
        .unwrap();
    SplitCluster {
        edges: public,
        admin: internal.directory("ops"),
        _pki: internal.pki,
    }
}

pub fn session_config() -> SessionConfig {
    SessionConfig {
        retry_min: Duration::from_millis(100),
        retry_max: Duration::from_millis(300),
        ..SessionConfig::default()
    }
}

pub async fn connect(url: &str, name: &str, locale: &str) -> Session {
    Client::register(url, name, "correct horse battery", locale, None)
        .await
        .unwrap()
        .connect_with(Arc::new(MockDeviceTranslator::default()), session_config())
        .await
        .unwrap()
}

/// Waits until `check` holds, looking again after every update.
pub async fn eventually(session: &Session, what: &str, check: impl Fn(&Session) -> bool) {
    let mut updates = session.updates();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !check(session) {
        match tokio::time::timeout_at(deadline, updates.recv()).await {
            Ok(_) => {}
            Err(_) => panic!("{what} never happened"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn people_on_different_edges_chat() {
    let cluster = start(2).await;
    let alice = connect(&cluster.edges[0], "alice", "ja-JP").await;
    let bob = connect(&cluster.edges[1], "bob", "en-US").await;

    let guild = alice.create_guild("Split Club", None).await.unwrap();
    alice
        .set_languages(babel_sdk::LanguagePolicyUpdate {
            guild_id: guild.guild.id.clone(),
            languages: vec!["ja".into(), "en".into()],
            llm_languages: vec!["ja".into(), "en".into()],
            region_mode: babel_sdk::RegionMode::Merged,
            aliases: Vec::new(),
            translate_regional_variants: false,
            model: None,
        })
        .await
        .unwrap();
    let channel = alice
        .create_channel(&guild.guild.id, "general", None)
        .await
        .unwrap();
    let invite = alice
        .create_invite(&guild.guild.id, None, None)
        .await
        .unwrap();
    bob.join(&invite.code).await.unwrap();
    bob.load_history(&channel.id, 50).await.unwrap();

    let sent = alice.send(&channel.id, "ただいま").await.unwrap();
    eventually(&bob, "Bob reading Alice in English", |bob| {
        bob.message(&channel.id, &sent.message.id)
            .is_some_and(|message| {
                matches!(message.display.shown, Shown::Server { .. })
                    && message.display.text == "[en-US] ただいま"
            })
    })
    .await;

    let reply = bob.send(&channel.id, "welcome back").await.unwrap();
    alice.load_history(&channel.id, 50).await.unwrap();
    eventually(&alice, "Alice reading Bob in Japanese", |alice| {
        alice
            .message(&channel.id, &reply.message.id)
            .is_some_and(|message| message.display.text == "[ja-JP] welcome back")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_users_devices_on_different_edges_stay_in_sync() {
    let cluster = start(2).await;
    let laptop = connect(&cluster.edges[0], "alice", "ja-JP").await;
    let phone = Client::login(&cluster.edges[1], "alice", "correct horse battery")
        .await
        .unwrap()
        .connect_with(Arc::new(MockDeviceTranslator::default()), session_config())
        .await
        .unwrap();
    let bob = connect(&cluster.edges[1], "bob", "en-US").await;

    // A guild made on the laptop reaches the phone through the other Edge.
    let guild = laptop.create_guild("Two Devices", None).await.unwrap();
    let guild_id = guild.guild.id.clone();
    eventually(&phone, "the phone hearing of the guild", |phone| {
        phone.guild(&guild_id).is_some()
    })
    .await;
    let channel = laptop
        .create_channel(&guild_id, "general", None)
        .await
        .unwrap();
    eventually(&phone, "the phone seeing the channel", |phone| {
        phone.channel(&channel.id).is_some()
    })
    .await;

    // Bob mentions Alice: unread on both devices. Reading it on the laptop
    // clears it on the phone.
    let invite = laptop.create_invite(&guild_id, None, None).await.unwrap();
    bob.join(&invite.code).await.unwrap();
    bob.send(&channel.id, "hi @alice").await.unwrap();
    for (device, session) in [("laptop", &laptop), ("phone", &phone)] {
        eventually(session, &format!("a mention on the {device}"), |session| {
            session.unread(&channel.id).mentions == 1
        })
        .await;
    }
    laptop.mark_read(&channel.id).await.unwrap();
    eventually(&phone, "the phone seeing it read", |phone| {
        !phone.unread(&channel.id).unread
    })
    .await;

    // So do notification settings.
    laptop
        .set_notifications(NotificationSettings {
            guild_id: guild_id.clone(),
            level: NotifyLevel::Mentions,
            channels: Vec::new(),
        })
        .await
        .unwrap();
    eventually(&phone, "the phone following the settings", |phone| {
        phone
            .notification_settings(&guild_id)
            .is_some_and(|settings| settings.level == NotifyLevel::Mentions)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn guilds_move_between_cells_while_people_chat() {
    let cluster = start(2).await;
    let alice = Arc::new(connect(&cluster.edges[0], "alice", "ja-JP").await);
    let bob = connect(&cluster.edges[1], "bob", "en-US").await;
    let guild = alice.create_guild("Moving Club", None).await.unwrap();
    let guild_id = guild.guild.id.clone();
    let channel = alice
        .create_channel(&guild_id, "general", None)
        .await
        .unwrap();
    let invite = alice.create_invite(&guild_id, None, None).await.unwrap();
    bob.join(&invite.code).await.unwrap();
    bob.load_history(&channel.id, 50).await.unwrap();
    let before = alice.send(&channel.id, "before the move").await.unwrap();

    // Alice keeps writing, as fast as she can, until the guild has moved
    // to the other cell and back.
    let moving = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let writer = {
        let (alice, moving) = (alice.clone(), moving.clone());
        let channel = channel.id.clone();
        tokio::spawn(async move {
            let mut sent = Vec::new();
            while moving.load(std::sync::atomic::Ordering::Relaxed) || sent.len() < 5 {
                let text = format!("during {}", sent.len());
                let message = alice.send(&channel, &text).await.unwrap();
                sent.push(message.message.id);
            }
            sent
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let home = cluster.admin.get_placement(guild_id.clone()).await.unwrap();
    let away = MESSAGE_CELLS
        .iter()
        .find(|cell| **cell != home.message_cell)
        .unwrap()
        .to_string();
    let moved = cluster
        .admin
        .move_guild(guild_id.clone(), away.clone())
        .await
        .unwrap();
    assert_eq!(moved.message_cell, away);
    assert_eq!(moved.epoch, home.epoch + 1);
    let back = cluster
        .admin
        .move_guild(guild_id.clone(), home.message_cell.clone())
        .await
        .unwrap();
    assert_eq!(back.message_cell, home.message_cell);
    moving.store(false, std::sync::atomic::Ordering::Relaxed);
    let sent = writer.await.unwrap();

    // Nothing written during the moves was lost, and Bob saw all of it live.
    for id in &sent {
        eventually(&bob, "Bob seeing every message", |bob| {
            bob.message(&channel.id, id).is_some()
        })
        .await;
    }
    // Old messages moved too: they can still be edited, and Bob hears it.
    alice
        .edit(&channel.id, &before.message.id, "before the moves")
        .await
        .unwrap();
    eventually(&bob, "Bob seeing the edit", |bob| {
        bob.message(&channel.id, &before.message.id)
            .is_some_and(|message| message.message.text == "before the moves")
    })
    .await;
    // And a fresh session reads the whole history from the cell it is on.
    let reader = Client::login(&cluster.edges[1], "bob", "correct horse battery")
        .await
        .unwrap()
        .connect_with(Arc::new(MockDeviceTranslator::default()), session_config())
        .await
        .unwrap();
    reader.load_history(&channel.id, 50).await.unwrap();
    reader.load_history(&channel.id, 100).await.unwrap();
    assert_eq!(reader.messages(&channel.id).len(), sent.len() + 1);
}
