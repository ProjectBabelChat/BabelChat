//! A live session: keeps state current and tells the app what changed.
//!
//! ```text
//! connect ──▶ Ready (guilds) ──▶ poll loop ──▶ events ──▶ state ──▶ Update
//!                                   │
//!                network error ─────┤ retry the same session (the Edge
//!                                   │ keeps its queue for a while)
//!                session gone ──────┘ connect again, reload guilds and
//!                                     the newest page of loaded channels
//! ```

use std::{collections::HashSet, sync::Arc, time::Duration};

use babel_api_edge_client::types::{
    ApiError, Channel, ChannelKind, GuildView, Invite, LanguagePolicy, LanguagePolicyUpdate,
    MessageEdit, NewChannel, NewGuild, NewInvite, NewMessage, UserSettings,
};
use babel_lang::Normalizer;
use parking_lot::Mutex;
use tokio::{
    runtime::Handle,
    sync::{Semaphore, broadcast},
    task::JoinHandle,
};

use crate::{
    Client, DeviceTranslator, Display, Error, Message, MessageTranslation, Result, Shown, device,
    display::{self, DeviceJob, Plan},
    state::{Entry, State},
};

#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// How long one poll waits for events on the server.
    pub poll_wait: Duration,
    /// Backoff between attempts while the server is unreachable.
    pub retry_min: Duration,
    pub retry_max: Duration,
    /// On-device translations running at once.
    pub device_concurrency: usize,
    /// Messages reloaded per open channel after a reconnect.
    pub reload_limit: i64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            poll_wait: Duration::from_secs(25),
            retry_min: Duration::from_millis(500),
            retry_max: Duration::from_secs(30),
            device_concurrency: 2,
            reload_limit: 50,
        }
    }
}

/// Something changed; read the new state from the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Guild {
        guild_id: String,
        change: GuildChange,
    },
    Message {
        guild_id: String,
        channel_id: String,
        message_id: String,
        change: MessageChange,
    },
    /// The server cannot be reached; the session keeps trying.
    Offline { error: String },
    /// Reachable again. With `reloaded`, the Edge had lost the session and
    /// everything was reloaded: redraw from scratch.
    Online { reloaded: bool },
    /// The token was rejected. The session has stopped; sign in again.
    SignedOut,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GuildChange {
    Joined,
    Left,
    MemberJoined {
        user_id: String,
    },
    ChannelCreated {
        channel_id: String,
    },
    /// The guild's languages, or how this user reads them.
    Languages,
    /// Reloaded after a reconnect or a settings change.
    Refreshed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageChange {
    Created,
    Edited,
    /// A translation arrived, from the server or the device: the display
    /// may have changed.
    Translated,
    Deleted,
}

/// A message as this reader should see it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub message: Message,
    pub translation: Option<MessageTranslation>,
    pub display: Display,
}

/// Device results kept before the cache is cleared.
const DEVICE_CACHE_LIMIT: usize = 10_000;
const UPDATE_CAPACITY: usize = 1024;

pub struct Session {
    shared: Arc<Shared>,
    poller: JoinHandle<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.poller.abort();
    }
}

struct Shared {
    client: Client,
    translator: Arc<dyn DeviceTranslator>,
    normalizer: Normalizer,
    config: SessionConfig,
    runtime: Handle,
    state: Mutex<State>,
    updates: broadcast::Sender<Update>,
    device_slots: Arc<Semaphore>,
    device_running: Mutex<HashSet<DeviceJob>>,
}

impl Session {
    pub(crate) async fn connect(
        client: Client,
        translator: Arc<dyn DeviceTranslator>,
        config: SessionConfig,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            device_slots: Arc::new(Semaphore::new(config.device_concurrency.max(1))),
            client,
            translator,
            normalizer: Normalizer::new(),
            config,
            runtime: Handle::current(),
            state: Mutex::default(),
            updates: broadcast::channel(UPDATE_CAPACITY).0,
            device_running: Mutex::default(),
        });
        let jwt = shared.client.jwt();
        let settings = shared.client.me().get_settings(jwt.clone()).await?;
        let ready = shared.client.gateway().connect(jwt).await?;
        {
            let mut state = shared.state.lock();
            state.settings = Some(settings);
            state.guilds = ready.guilds;
        }
        let poller = tokio::spawn(shared.clone().run(ready.session_id, ready.cursor));
        Ok(Self { shared, poller })
    }

    /// Updates from now on. Each receiver sees every update; one that falls
    /// more than 1024 behind gets `Lagged` and should redraw everything.
    pub fn updates(&self) -> broadcast::Receiver<Update> {
        self.shared.updates.subscribe()
    }

    pub fn client(&self) -> &Client {
        &self.shared.client
    }

    pub fn user_id(&self) -> &str {
        self.shared.client.user_id()
    }

    pub fn user_name(&self) -> &str {
        self.shared.client.user_name()
    }

    pub fn settings(&self) -> UserSettings {
        self.shared
            .state
            .lock()
            .settings
            .clone()
            .expect("set on connect")
    }

    pub fn guilds(&self) -> Vec<GuildView> {
        self.shared.state.lock().guilds.clone()
    }

    pub fn guild(&self, guild_id: &str) -> Option<GuildView> {
        self.shared.state.lock().guild(guild_id).cloned()
    }

    pub fn channel(&self, channel_id: &str) -> Option<Channel> {
        self.shared.state.lock().channel(channel_id).cloned()
    }

    /// Loaded messages of a channel, oldest first.
    pub fn messages(&self, channel_id: &str) -> Vec<ChatMessage> {
        let state = self.shared.state.lock();
        let Some(log) = state.channels.get(channel_id) else {
            return Vec::new();
        };
        log.messages
            .values()
            .map(|entry| self.shared.chat_message(&state, entry))
            .collect()
    }

    pub fn message(&self, channel_id: &str, message_id: &str) -> Option<ChatMessage> {
        let state = self.shared.state.lock();
        let entry = state.entry(channel_id, message_id)?;
        Some(self.shared.chat_message(&state, entry))
    }

    /// Whether the oldest message of the channel is loaded.
    pub fn history_complete(&self, channel_id: &str) -> bool {
        self.shared
            .state
            .lock()
            .channels
            .get(channel_id)
            .is_some_and(|log| log.complete)
    }

    /// Loads up to `limit` messages older than the oldest loaded one (the
    /// newest ones, the first time). Returns how many arrived.
    pub async fn load_history(&self, channel_id: &str, limit: i64) -> Result<usize> {
        let guild_id = self.shared.guild_of(channel_id)?;
        let before = {
            let state = self.shared.state.lock();
            state
                .channels
                .get(channel_id)
                .filter(|log| log.loaded)
                .and_then(|log| log.messages.values().next())
                .map(|entry| entry.message.id.clone())
        };
        self.shared
            .fetch(&guild_id, channel_id, before, limit, false)
            .await
    }

    pub async fn send(&self, channel_id: &str, text: &str) -> Result<ChatMessage> {
        let guild_id = self.shared.guild_of(channel_id)?;
        let view = self
            .shared
            .client
            .messages()
            .send_message(
                self.shared.client.jwt(),
                NewMessage {
                    guild_id,
                    channel_id: channel_id.to_string(),
                    text: text.to_string(),
                },
            )
            .await?;
        Ok(self.shared.put_own(view.message))
    }

    pub async fn edit(
        &self,
        channel_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<ChatMessage> {
        let guild_id = self.shared.guild_of(channel_id)?;
        let view = self
            .shared
            .client
            .messages()
            .edit_message(
                self.shared.client.jwt(),
                MessageEdit {
                    guild_id,
                    channel_id: channel_id.to_string(),
                    message_id: message_id.to_string(),
                    text: text.to_string(),
                },
            )
            .await?;
        Ok(self.shared.put_own(view.message))
    }

    pub async fn delete(&self, channel_id: &str, message_id: &str) -> Result<()> {
        let guild_id = self.shared.guild_of(channel_id)?;
        self.shared
            .client
            .messages()
            .delete_message(
                self.shared.client.jwt(),
                guild_id.clone(),
                channel_id.to_string(),
                message_id.to_string(),
            )
            .await?;
        if self
            .shared
            .state
            .lock()
            .remove_message(channel_id, message_id)
        {
            self.shared.emit(Update::Message {
                guild_id,
                channel_id: channel_id.to_string(),
                message_id: message_id.to_string(),
                change: MessageChange::Deleted,
            });
        }
        Ok(())
    }

    pub async fn create_guild(&self, name: &str, description: Option<&str>) -> Result<GuildView> {
        let view = self
            .shared
            .client
            .guilds()
            .create_guild(
                self.shared.client.jwt(),
                NewGuild {
                    name: name.to_string(),
                    description: description.map(str::to_string),
                },
            )
            .await?;
        self.shared.put_guild(view.clone());
        Ok(view)
    }

    /// Joins with an invite code someone shared.
    pub async fn join(&self, invite: &str) -> Result<GuildView> {
        let view = self
            .shared
            .client
            .guilds()
            .join_guild(self.shared.client.jwt(), invite.trim().to_string())
            .await?;
        self.shared.put_guild(view.clone());
        Ok(view)
    }

    /// `max_age_secs`: `None` for the server's default (7 days), `Some(0)`
    /// for never.
    pub async fn create_invite(
        &self,
        guild_id: &str,
        max_age_secs: Option<i64>,
        max_uses: Option<i64>,
    ) -> Result<Invite> {
        Ok(self
            .shared
            .client
            .guilds()
            .create_invite(
                self.shared.client.jwt(),
                NewInvite {
                    guild_id: guild_id.to_string(),
                    max_age_secs,
                    max_uses,
                },
            )
            .await?)
    }

    pub async fn revoke_invite(&self, code: &str) -> Result<()> {
        Ok(self
            .shared
            .client
            .guilds()
            .revoke_invite(self.shared.client.jwt(), code.trim().to_string())
            .await?)
    }

    pub async fn create_channel(
        &self,
        guild_id: &str,
        name: &str,
        topic: Option<&str>,
    ) -> Result<Channel> {
        self.shared
            .create_channel(NewChannel {
                guild_id: guild_id.to_string(),
                kind: ChannelKind::Text,
                name: name.to_string(),
                topic: topic.map(str::to_string),
                parent_id: None,
                starter_message_id: None,
            })
            .await
    }

    /// Starts a thread from a message. The thread's translations inherit
    /// the channel's context up to that message.
    pub async fn start_thread(
        &self,
        channel_id: &str,
        message_id: &str,
        name: &str,
    ) -> Result<Channel> {
        let guild_id = self.shared.guild_of(channel_id)?;
        self.shared
            .create_channel(NewChannel {
                guild_id,
                kind: ChannelKind::Thread,
                name: name.to_string(),
                topic: None,
                parent_id: Some(channel_id.to_string()),
                starter_message_id: Some(message_id.to_string()),
            })
            .await
    }

    /// Owner only. Tags are normalized by the server.
    pub async fn set_languages(&self, update: LanguagePolicyUpdate) -> Result<LanguagePolicy> {
        let guild_id = update.guild_id.clone();
        let policy = self
            .shared
            .client
            .guilds()
            .put_language_policy(self.shared.client.jwt(), update)
            .await?;
        self.shared.refresh_guild(&guild_id).await?;
        Ok(policy)
    }

    /// Saves settings, then reloads what depends on them: how each guild is
    /// read, and the translations of loaded messages.
    pub async fn set_settings(&self, settings: UserSettings) -> Result<UserSettings> {
        let settings = self
            .shared
            .client
            .me()
            .put_settings(self.shared.client.jwt(), settings)
            .await?;
        self.shared.state.lock().settings = Some(settings.clone());
        let guilds = self
            .guilds()
            .into_iter()
            .map(|view| view.guild.id)
            .collect::<Vec<_>>();
        for guild_id in guilds {
            self.shared.refresh_guild(&guild_id).await?;
        }
        self.shared.reload_channels().await;
        Ok(settings)
    }
}

impl Shared {
    fn emit(&self, update: Update) {
        let _ = self.updates.send(update);
    }

    fn guild_of(&self, channel_id: &str) -> Result<String> {
        self.state
            .lock()
            .channel(channel_id)
            .map(|channel| channel.guild_id.clone())
            .ok_or(Error::Api(ApiError::NotFound))
    }

    async fn run(self: Arc<Self>, session_id: String, cursor: i64) {
        let mut session = Some((session_id, cursor));
        let mut backoff = self.config.retry_min;
        let mut offline = false;
        loop {
            let result = match &session {
                Some((id, cursor)) => self
                    .client
                    .gateway()
                    .poll(
                        self.client.jwt(),
                        id.clone(),
                        *cursor,
                        self.config.poll_wait.as_millis() as i64,
                    )
                    .await
                    .map(Some)
                    .map_err(Error::from),
                None => self.reconnect().await.map(|ready| {
                    session = Some(ready);
                    None
                }),
            };
            match result {
                Ok(batch) => {
                    let reloaded = batch.is_none();
                    if offline || reloaded {
                        self.emit(Update::Online { reloaded });
                    }
                    offline = false;
                    backoff = self.config.retry_min;
                    if let (Some(batch), Some((_, cursor))) = (batch, &mut session) {
                        for event in batch.events {
                            self.apply(event);
                        }
                        *cursor = batch.next;
                    }
                }
                Err(Error::Unauthorized) => {
                    self.emit(Update::SignedOut);
                    return;
                }
                // The Edge lost the session (restart, or we fell too far
                // behind): start over at once.
                Err(Error::Api(ApiError::NotFound)) if session.is_some() => session = None,
                Err(error) => {
                    if !offline {
                        tracing::warn!(%error, "BabelChat is unreachable; retrying");
                        self.emit(Update::Offline {
                            error: error.to_string(),
                        });
                        offline = true;
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(self.config.retry_max);
                }
            }
        }
    }

    fn apply(self: &Arc<Self>, event: babel_api_edge_client::types::Event) {
        let updates = self.state.lock().apply(event);
        for update in updates {
            if let Update::Message {
                channel_id,
                message_id,
                change: MessageChange::Created | MessageChange::Edited | MessageChange::Translated,
                ..
            } = &update
            {
                self.prepare(channel_id, message_id);
            }
            self.emit(update);
        }
    }

    /// Starts device translation early, before the app asks.
    fn prepare(self: &Arc<Self>, channel_id: &str, message_id: &str) {
        let state = self.state.lock();
        if let Some(entry) = state.entry(channel_id, message_id) {
            self.chat_message(&state, entry);
        }
    }

    /// New session after the old one was lost: guilds from the new Ready,
    /// loaded channels from their newest page.
    async fn reconnect(self: &Arc<Self>) -> Result<(String, i64)> {
        let jwt = self.client.jwt();
        let settings = self.client.me().get_settings(jwt.clone()).await?;
        let ready = self.client.gateway().connect(jwt).await?;
        let (joined, left) = {
            let mut state = self.state.lock();
            state.settings = Some(settings);
            let known = state
                .guilds
                .iter()
                .map(|view| view.guild.id.clone())
                .collect::<HashSet<_>>();
            let now = ready
                .guilds
                .iter()
                .map(|view| view.guild.id.clone())
                .collect::<HashSet<_>>();
            state.guilds = ready.guilds;
            state.channels.retain(|_, log| now.contains(&log.guild_id));
            (now, known)
        };
        for guild_id in left.difference(&joined) {
            self.emit(Update::Guild {
                guild_id: guild_id.clone(),
                change: GuildChange::Left,
            });
        }
        for guild_id in &joined {
            self.emit(Update::Guild {
                guild_id: guild_id.clone(),
                change: if left.contains(guild_id) {
                    GuildChange::Refreshed
                } else {
                    GuildChange::Joined
                },
            });
        }
        self.reload_channels().await;
        Ok((ready.session_id, ready.cursor))
    }

    /// Reloads the newest page of every channel the app has opened.
    async fn reload_channels(self: &Arc<Self>) {
        let loaded = {
            let state = self.state.lock();
            state
                .channels
                .iter()
                .filter(|(_, log)| log.loaded)
                .map(|(channel, log)| (log.guild_id.clone(), channel.clone()))
                .collect::<Vec<_>>()
        };
        for (guild_id, channel_id) in loaded {
            if let Err(error) = self
                .fetch(&guild_id, &channel_id, None, self.config.reload_limit, true)
                .await
            {
                tracing::debug!(%error, channel_id, "could not reload a channel");
            }
        }
    }

    async fn fetch(
        self: &Arc<Self>,
        guild_id: &str,
        channel_id: &str,
        before: Option<String>,
        limit: i64,
        authoritative: bool,
    ) -> Result<usize> {
        let page = self
            .client
            .messages()
            .get_messages(
                self.client.jwt(),
                guild_id.to_string(),
                channel_id.to_string(),
                before,
                limit,
            )
            .await?;
        let ids = page
            .iter()
            .map(|view| view.message.id.clone())
            .collect::<Vec<_>>();
        let count = self.state.lock().merge_page(
            guild_id,
            channel_id,
            page.into_iter()
                .map(|view| (view.message, view.translation))
                .collect(),
            limit as usize,
            authoritative,
        );
        for id in ids {
            self.prepare(channel_id, &id);
        }
        Ok(count)
    }

    fn put_own(self: &Arc<Self>, message: Message) -> ChatMessage {
        let (guild_id, channel_id, message_id) = (
            message.guild_id.clone(),
            message.channel_id.clone(),
            message.id.clone(),
        );
        let change = self.state.lock().put_message(message);
        if let Some(change) = change {
            self.emit(Update::Message {
                guild_id,
                channel_id: channel_id.clone(),
                message_id: message_id.clone(),
                change,
            });
        }
        let state = self.state.lock();
        let entry = state.entry(&channel_id, &message_id).expect("just stored");
        self.chat_message(&state, entry)
    }

    fn put_guild(&self, view: GuildView) {
        let guild_id = view.guild.id.clone();
        let added = self.state.lock().put_guild(view);
        self.emit(Update::Guild {
            guild_id,
            change: if added {
                GuildChange::Joined
            } else {
                GuildChange::Refreshed
            },
        });
    }

    async fn refresh_guild(&self, guild_id: &str) -> Result<()> {
        let view = self
            .client
            .guilds()
            .get_guild(self.client.jwt(), guild_id.to_string())
            .await?;
        self.state.lock().put_guild(view);
        self.emit(Update::Guild {
            guild_id: guild_id.to_string(),
            change: GuildChange::Languages,
        });
        Ok(())
    }

    async fn create_channel(&self, new: NewChannel) -> Result<Channel> {
        let channel = self
            .client
            .guilds()
            .create_channel(self.client.jwt(), new)
            .await?;
        let added = {
            let mut state = self.state.lock();
            state
                .guilds
                .iter_mut()
                .find(|view| view.guild.id == channel.guild_id)
                .filter(|view| view.channels.iter().all(|known| known.id != channel.id))
                .map(|view| view.channels.push(channel.clone()))
                .is_some()
        };
        if added {
            self.emit(Update::Guild {
                guild_id: channel.guild_id.clone(),
                change: GuildChange::ChannelCreated {
                    channel_id: channel.id.clone(),
                },
            });
        }
        Ok(channel)
    }

    /// The message with its display, starting device translation if the
    /// display needs it.
    fn chat_message(self: &Arc<Self>, state: &State, entry: &Entry) -> ChatMessage {
        let message = &entry.message;
        let display = match state.guild(&message.guild_id) {
            None => Display {
                text: message.text.clone(),
                shown: Shown::Original,
            },
            Some(view) => match display::plan(
                &self.normalizer,
                self.client.user_id(),
                message,
                entry.translation.as_ref(),
                &view.language,
            ) {
                Plan::Show(display) => display,
                Plan::Device(job) => {
                    let result = state.device.get(&job);
                    if result.is_none() {
                        self.start_device(job.clone(), message);
                    }
                    display::device_display(&self.normalizer, message, &job, result)
                }
            },
        };
        ChatMessage {
            message: message.clone(),
            translation: entry.translation.clone(),
            display,
        }
    }

    fn start_device(self: &Arc<Self>, job: DeviceJob, message: &Message) {
        if !self.device_running.lock().insert(job.clone()) {
            return;
        }
        let shared = self.clone();
        let (guild_id, channel_id) = (message.guild_id.clone(), message.channel_id.clone());
        self.runtime.spawn(async move {
            let result = {
                let _slot = shared.device_slots.clone().acquire_owned().await;
                device::translate(
                    &*shared.translator,
                    &shared.normalizer,
                    &job.text,
                    &job.from,
                    &job.to,
                )
                .await
                .map_err(|error| error.to_string())
            };
            {
                let mut state = shared.state.lock();
                if state.device.len() >= DEVICE_CACHE_LIMIT {
                    state.device.clear();
                }
                state.device.insert(job.clone(), result);
            }
            shared.device_running.lock().remove(&job);
            shared.emit(Update::Message {
                guild_id,
                channel_id,
                message_id: job.message_id,
                change: MessageChange::Translated,
            });
        });
    }
}
