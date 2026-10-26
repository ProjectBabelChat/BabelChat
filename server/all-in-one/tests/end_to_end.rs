//! The whole system over HTTP, through the generated public client: what a
//! BabelChat app sees.

use std::time::Duration;

use babel_api_edge_client::types::{
    ApiError, Auth, ChannelKind, EventKind, Gateway, Guilds, LanguagePolicyUpdate, Me, Messages,
    NewChannel, NewGuild, NewMessage, RegionMode, Registration, ThrowsOrOtherError,
};
use babel_api_translation_cell::types::Plan;
use babel_server::{EngineChoice, ServerConfig, start};

struct Client {
    url: String,
    token: String,
    user_id: String,
}

impl Client {
    async fn register(url: &str, name: &str, locale: &str) -> Self {
        let token = Auth::new(url)
            .register(Registration {
                name: name.into(),
                password: "correct horse battery".into(),
                locale: locale.into(),
            })
            .await
            .unwrap();
        Self {
            url: url.to_string(),
            token: token.jwt,
            user_id: token.user_id,
        }
    }

    fn guilds(&self) -> Guilds {
        Guilds::new(&self.url)
    }

    fn messages(&self) -> Messages {
        Messages::new(&self.url)
    }

    fn gateway(&self) -> Gateway {
        Gateway::new(&self.url)
    }
}

fn config(dir: &std::path::Path) -> ServerConfig {
    ServerConfig {
        data_dir: dir.to_path_buf(),
        message_cells: 2,
        translation_cells: 2,
        engine: EngineChoice::Mock {
            latency: Duration::from_millis(20),
        },
        default_plan: Plan::Paid3,
        daily_budget: None,
    }
}

const JAPANESE: &str = "明日の夜に駅の近くでみんなと集まって一緒に遊びませんか";

#[tokio::test]
async fn messages_reach_every_reader_in_their_language() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _server) = start(&config(dir.path()), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{address}");

    let alice = Client::register(&url, "alice", "ja-JP").await;
    let bob = Client::register(&url, "bob", "en-GB").await;
    let carol = Client::register(&url, "carol", "fr-FR").await;

    // Alice creates a guild; it starts with her language.
    let guild = alice
        .guilds()
        .create_guild(
            alice.token.clone(),
            NewGuild {
                name: "Rhythm Club".into(),
                description: Some("We play rhythm games together.".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(guild.policy.languages, ["ja-JP"]);
    assert!(guild.language.llm);
    let guild_id = guild.guild.id.clone();

    // She declares Japanese, English and Korean; the LLM covers two.
    let policy = alice
        .guilds()
        .put_language_policy(
            alice.token.clone(),
            LanguagePolicyUpdate {
                guild_id: guild_id.clone(),
                languages: vec!["ja".into(), "en_GB".into(), "ko".into()],
                llm_languages: vec!["ja".into(), "en".into()],
                region_mode: RegionMode::Merged,
                aliases: Vec::new(),
                translate_regional_variants: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(policy.languages, ["ja-JP", "en-US", "ko-KR"]);

    let channel = alice
        .guilds()
        .create_channel(
            alice.token.clone(),
            NewChannel {
                guild_id: guild_id.clone(),
                kind: ChannelKind::Text,
                name: "general".into(),
                topic: None,
                parent_id: None,
                starter_message_id: None,
            },
        )
        .await
        .unwrap();

    // Only the owner may change the policy.
    bob.guilds()
        .join_guild(bob.token.clone(), guild_id.clone())
        .await
        .unwrap();
    let denied = bob
        .guilds()
        .put_language_policy(
            bob.token.clone(),
            LanguagePolicyUpdate {
                guild_id: guild_id.clone(),
                languages: vec!["en".into()],
                llm_languages: Vec::new(),
                region_mode: RegionMode::Merged,
                aliases: Vec::new(),
                translate_regional_variants: false,
            },
        )
        .await;
    assert!(matches!(
        denied,
        Err(ThrowsOrOtherError::Throws(ApiError::NotFound))
    ));

    carol
        .guilds()
        .join_guild(carol.token.clone(), guild_id.clone())
        .await
        .unwrap();

    // Bob (en-GB) reads English from the LLM; Carol (fr-FR) is outside the
    // set and translates on device from the English pivot.
    let bob_ready = bob.gateway().connect(bob.token.clone()).await.unwrap();
    let bob_view = &bob_ready.guilds[0].language;
    assert_eq!(bob_view.effective, "en-US");
    assert!(bob_view.llm && !bob_view.client_translation);

    let carol_ready = carol.gateway().connect(carol.token.clone()).await.unwrap();
    let carol_view = &carol_ready.guilds[0].language;
    assert_eq!(carol_view.effective, "fr-FR");
    assert!(!carol_view.in_set && carol_view.client_translation);

    let sent = alice
        .messages()
        .send_message(
            alice.token.clone(),
            NewMessage {
                guild_id: guild_id.clone(),
                channel_id: channel.id.clone(),
                text: JAPANESE.into(),
            },
        )
        .await
        .unwrap();

    // Both see the message, then its translation.
    for (client, ready) in [(&bob, &bob_ready), (&carol, &carol_ready)] {
        let mut after = ready.cursor;
        let mut created = false;
        let mut translation = None;
        while translation.is_none() {
            let batch = client
                .gateway()
                .poll(client.token.clone(), ready.session_id.clone(), after, 5_000)
                .await
                .unwrap();
            assert!(!batch.events.is_empty(), "no events for {}", client.user_id);
            for event in batch.events {
                match event.kind {
                    EventKind::MessageCreated => {
                        assert_eq!(event.message.unwrap().id, sent.message.id);
                        created = true;
                    }
                    EventKind::MessageTranslated => translation = event.translation,
                    _ => {}
                }
            }
            after = batch.next;
        }
        assert!(created);
        let translation = translation.unwrap();
        assert_eq!(translation.source_lang, "ja-JP");
        assert_eq!(translation.status, "done");
        // Only English reaches both: Bob's language, and Carol's pivot.
        assert_eq!(translation.translations.len(), 1);
        assert_eq!(translation.translations[0].lang, "en-US");
        assert!(translation.translations[0].text.contains(JAPANESE));
    }

    // History comes joined with translations.
    let history = bob
        .messages()
        .get_messages(
            bob.token.clone(),
            guild_id.clone(),
            channel.id.clone(),
            None,
            50,
        )
        .await
        .unwrap();
    assert_eq!(history[0].message.id, sent.message.id);
    assert_eq!(
        history[0].translation.as_ref().unwrap().translations[0].lang,
        "en-US"
    );

    // Settings follow the user.
    let settings = Me::new(&url)
        .get_settings(carol.token.clone())
        .await
        .unwrap();
    assert_eq!(settings.locale, "fr-FR");
}

#[tokio::test]
async fn guild_ids_cannot_be_probed() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _server) = start(&config(dir.path()), "127.0.0.1:0").await.unwrap();
    let url = format!("http://{address}");

    let alice = Client::register(&url, "alice", "ja-JP").await;
    let mallory = Client::register(&url, "mallory", "en-US").await;
    let guild = alice
        .guilds()
        .create_guild(
            alice.token.clone(),
            NewGuild {
                name: "Private".into(),
                description: None,
            },
        )
        .await
        .unwrap();

    // An existing guild Mallory is not in, and a guild that does not exist,
    // answer the same way.
    for guild_id in [
        guild.guild.id.clone(),
        "0123456789abcdef01234567".to_string(),
    ] {
        let result = mallory
            .guilds()
            .get_guild(mallory.token.clone(), guild_id)
            .await;
        assert!(matches!(
            result,
            Err(ThrowsOrOtherError::Throws(ApiError::NotFound))
        ));
    }

    // Nothing in the public answer names a cell.
    let json = serde_json::to_string(&guild).unwrap();
    assert!(!json.contains("cell"), "{json}");

    // Tokens are required.
    let anonymous = Guilds::new(&url)
        .get_guild("not-a-token", guild.guild.id)
        .await;
    assert!(matches!(anonymous, Err(ThrowsOrOtherError::Other(_))));
}

#[tokio::test]
async fn data_survives_a_crash() {
    let dir = tempfile::tempdir().unwrap();

    let (mut server, url) = spawn_server(dir.path()).await;
    let alice = Client::register(&url, "alice", "ja-JP").await;
    let guild_id = alice
        .guilds()
        .create_guild(
            alice.token.clone(),
            NewGuild {
                name: "Persistent".into(),
                description: None,
            },
        )
        .await
        .unwrap()
        .guild
        .id;

    // Kill it like a crash, then start again on the same data.
    server.kill().await.unwrap();
    let (_server, url) = spawn_server(dir.path()).await;

    let ready = Gateway::new(&url).connect(alice.token).await.unwrap();
    assert_eq!(ready.guilds[0].guild.id, guild_id);
}

/// Starts the real `babel-server` binary on a free port and returns it with
/// its URL once it accepts connections.
async fn spawn_server(data: &std::path::Path) -> (tokio::process::Child, String) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_babel-server"))
        .args(["--listen", "127.0.0.1:0", "--plan", "paid3", "--data"])
        .arg(data)
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let url = tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(url) = line.strip_prefix("listening on ") {
                return url.to_string();
            }
        }
        panic!("the server exited before listening");
    })
    .await
    .expect("the server never came up");

    // Keep draining its output so it never blocks on a full pipe.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
    (child, url)
}
