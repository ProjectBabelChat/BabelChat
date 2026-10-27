//! Helpers shared by the SDK's tests: a whole in-process server, and users
//! with live sessions on it.

#![allow(dead_code)]

use std::{sync::Arc, time::Duration};

use babel_api_translation_cell::types::Plan;
use babel_sdk::{
    ChatMessage, Client, LanguagePolicyUpdate, MockDeviceTranslator, Notification, RegionMode,
    Session, SessionConfig, Update,
};
use babel_server::{EngineChoice, ServerConfig, start};
use tokio::{sync::broadcast, task::JoinHandle};

pub const PASSWORD: &str = "correct horse battery";
pub const JAPANESE: &str = "明日の夜に駅の近くでみんなと集まって一緒に遊びませんか";

pub struct Server {
    pub url: String,
    pub task: JoinHandle<anyhow::Result<()>>,
    _dir: tempfile::TempDir,
}

pub async fn server(plan: Plan) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let config = ServerConfig {
        data_dir: dir.path().to_path_buf(),
        message_cells: 2,
        translation_cells: 2,
        engine: EngineChoice::Mock {
            latency: Duration::from_millis(20),
        },
        default_plan: plan,
        daily_budget: None,
    };
    let (address, task) = start(&config, "127.0.0.1:0").await.unwrap();
    Server {
        url: format!("http://{address}"),
        task,
        _dir: dir,
    }
}

pub struct User {
    pub session: Session,
    pub updates: broadcast::Receiver<Update>,
}

pub async fn user(server: &Server, name: &str, locale: &str) -> User {
    let client = Client::register(&server.url, name, PASSWORD, locale, None)
        .await
        .unwrap();
    connect(&client, SessionConfig::default()).await
}

/// Another session of a user that exists already (a second device).
pub async fn sign_in(server: &Server, name: &str, config: SessionConfig) -> User {
    let client = Client::login(&server.url, name, PASSWORD).await.unwrap();
    connect(&client, config).await
}

pub async fn connect(client: &Client, config: SessionConfig) -> User {
    let session = client
        .connect_with(Arc::new(MockDeviceTranslator::default()), config)
        .await
        .unwrap();
    let updates = session.updates();
    User { session, updates }
}

impl User {
    /// Waits until `check` finds what it looks for in the session.
    pub async fn until<T>(&mut self, mut check: impl FnMut(&Session) -> Option<T>) -> T {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = check(&self.session) {
                return found;
            }
            match tokio::time::timeout_at(deadline, self.updates.recv()).await {
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(error)) => panic!("updates ended: {error}"),
                Err(_) => panic!("timed out"),
            }
        }
    }

    /// The next update `pick` accepts, skipping the others.
    pub async fn next<T>(&mut self, mut pick: impl FnMut(Update) -> Option<T>) -> T {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            match tokio::time::timeout_at(deadline, self.updates.recv()).await {
                Ok(Ok(update)) => {
                    if let Some(found) = pick(update) {
                        return found;
                    }
                }
                Ok(Err(error)) => panic!("updates ended: {error}"),
                Err(_) => panic!("timed out"),
            }
        }
    }

    pub async fn notification(&mut self) -> Notification {
        self.next(|update| match update {
            Update::Notification(notification) => Some(notification),
            _ => None,
        })
        .await
    }

    /// The message once its display has settled.
    pub async fn settled(&mut self, channel: &str, id: &str) -> ChatMessage {
        self.until(|session| {
            session
                .message(channel, id)
                .filter(|message| message.display.is_settled())
        })
        .await
    }
}

/// The owner's guild with one channel, joined by everyone else through an
/// invite. Returns the channel ID.
pub async fn guild(
    owner: &mut User,
    others: &mut [&mut User],
    languages: &[&str],
    llm: &[&str],
) -> String {
    let guild = owner
        .session
        .create_guild("Rhythm Club", Some("We play rhythm games together."))
        .await
        .unwrap();
    let guild_id = guild.guild.id.clone();
    owner
        .session
        .set_languages(LanguagePolicyUpdate {
            guild_id: guild_id.clone(),
            languages: languages.iter().map(|tag| tag.to_string()).collect(),
            llm_languages: llm.iter().map(|tag| tag.to_string()).collect(),
            region_mode: RegionMode::Merged,
            aliases: Vec::new(),
            translate_regional_variants: false,
            model: None,
        })
        .await
        .unwrap();
    let channel = owner
        .session
        .create_channel(&guild_id, "general", None)
        .await
        .unwrap();
    let invite = owner
        .session
        .create_invite(&guild_id, None, None)
        .await
        .unwrap();
    for other in others {
        let joined = other.session.join(&invite.code).await.unwrap();
        assert_eq!(joined.guild.id, guild_id);
    }
    channel.id
}
