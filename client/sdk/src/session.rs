//! A live session: keeps state current and tells the app what changed.
//!
//! ```text
//! connect ──▶ Ready (guilds) ──▶ events (WebSocket, or long polls) ──▶ state ──▶ Update
//!                                   │
//!                network error ─────┤ resume the same session (the Edge
//!                                   │ keeps its queue for a while)
//!                session gone ──────┘ connect again, reload guilds and
//!                                     the newest page of loaded channels
//! ```
//!
//! With a cache file, a session starts from what was saved last time and
//! can start offline; it then connects as if reconnecting.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use babel_api_edge_client::types::{
    ApiError, Ban, Channel, ChannelKind, GuildView, Invite, LanguagePolicy, LanguagePolicyUpdate,
    MessageEdit, NewChannel, NewGuild, NewInvite, NewMessage, NotificationSettings,
    TranslationModel, UserSettings,
};
use babel_lang::Normalizer;
use parking_lot::Mutex;
use tokio::{
    runtime::Handle,
    sync::{Semaphore, broadcast},
    task::JoinHandle,
};

use crate::{
    Client, DeviceError, DeviceTranslator, Display, Error, Message, MessageTranslation, Result,
    Shown, cache, device,
    display::{self, DeviceJob, Plan},
    state::{Entry, State},
    websocket,
};

/// How events reach the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// WebSocket, falling back to long polling if the server has none.
    Auto,
    WebSocket,
    LongPoll,
}

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
    pub transport: Transport,
    /// How long a notification waits for its message's translation before
    /// it is sent with whatever is there.
    pub notify_wait: Duration,
    /// How long new messages in the focused channel wait before they are
    /// marked read, so a burst of them costs one request.
    pub read_delay: Duration,
    /// Where to keep state between runs. `None`: nothing is written.
    pub cache: Option<PathBuf>,
    /// How often the cache is written while something changes.
    pub cache_interval: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            poll_wait: Duration::from_secs(25),
            retry_min: Duration::from_millis(500),
            retry_max: Duration::from_secs(30),
            device_concurrency: 2,
            reload_limit: 50,
            transport: Transport::Auto,
            notify_wait: Duration::from_secs(3),
            read_delay: Duration::from_millis(500),
            cache: None,
            cache_interval: Duration::from_secs(5),
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
    /// The channel's unread marker or mention count changed
    /// ([`Session::unread`]).
    Unread {
        guild_id: String,
        channel_id: String,
    },
    /// Something the user asked to hear about, ready to show: its
    /// translation has arrived, or `notify_wait` has passed.
    Notification(Notification),
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
    /// The user left (here or on another device), or the guild vanished
    /// while the session was away.
    Left,
    /// The user was kicked or banned.
    Removed,
    MemberJoined {
        user_id: String,
    },
    MemberLeft {
        user_id: String,
    },
    /// Kicked or banned.
    MemberRemoved {
        user_id: String,
    },
    ChannelCreated {
        channel_id: String,
    },
    /// The guild's languages, or how this user reads them.
    Languages,
    /// The user's notification settings for the guild.
    Notifications,
    /// Reloaded after a reconnect or a settings change.
    Refreshed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Whether a channel has messages the user has not read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unread {
    pub unread: bool,
    /// Unread messages that mention the user (or reply to them).
    pub mentions: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyReason {
    /// Named with `@name`.
    Mention,
    /// A reply to one of the user's messages.
    Reply,
    /// Any message, in a guild or channel set to notify of everything.
    Message,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub guild_id: String,
    pub reason: NotifyReason,
    /// As the reader sees it when the notification fires.
    pub message: ChatMessage,
}

/// Device results kept before the cache is cleared.
const DEVICE_CACHE_LIMIT: usize = 10_000;
/// How long a failed device translation waits before it may be tried again.
const DEVICE_RETRY: Duration = Duration::from_secs(30);
const UPDATE_CAPACITY: usize = 1024;

pub struct Session {
    shared: Arc<Shared>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.shared.save_cache();
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
    /// Counts updates, so the cache knows when to write.
    changes: AtomicU64,
    device_slots: Arc<Semaphore>,
    device_running: Mutex<HashSet<DeviceJob>>,
    /// The channel on screen: it does not notify, and is read as it grows.
    focus: Mutex<Option<String>>,
    /// Channels with a read marker on its way.
    reading: Mutex<HashSet<String>>,
    websocket: AtomicBool,
    online: AtomicBool,
}

impl Session {
    pub(crate) async fn connect(
        client: Client,
        translator: Arc<dyn DeviceTranslator>,
        config: SessionConfig,
    ) -> Result<Self> {
        let cached = config
            .cache
            .as_deref()
            .and_then(|path| cache::load(path, client.server(), client.user_id()));
        let has_cache = cached.is_some();
        let mut state = cached.unwrap_or_default();
        state.user_id = client.user_id().to_string();

        let shared = Arc::new(Shared {
            device_slots: Arc::new(Semaphore::new(config.device_concurrency.max(1))),
            client,
            translator,
            normalizer: Normalizer::new(),
            config,
            runtime: Handle::current(),
            state: Mutex::new(state),
            updates: broadcast::channel(UPDATE_CAPACITY).0,
            changes: AtomicU64::new(0),
            device_running: Mutex::default(),
            focus: Mutex::default(),
            reading: Mutex::default(),
            websocket: AtomicBool::new(false),
            online: AtomicBool::new(false),
        });

        // With a cache, the app starts from it at once (offline too) and
        // the session connects in the background as if reconnecting.
        let session = if has_cache {
            None
        } else {
            let jwt = shared.client.jwt();
            let settings = shared.client.me().get_settings(jwt.clone()).await?;
            let ready = shared.client.gateway().connect(jwt).await?;
            let mut state = shared.state.lock();
            state.settings = Some(settings);
            state.guilds = ready.guilds;
            Some((ready.session_id, ready.cursor))
        };
        shared.online.store(session.is_some(), Ordering::Relaxed);

        let mut tasks = vec![tokio::spawn(shared.clone().run(session))];
        if shared.config.cache.is_some() {
            tasks.push(tokio::spawn(shared.clone().keep_cache()));
        }
        Ok(Self { shared, tasks })
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

    /// The user's settings; defaults until the server has been reached.
    pub fn settings(&self) -> UserSettings {
        self.shared
            .state
            .lock()
            .settings
            .clone()
            .unwrap_or(UserSettings {
                locale: "en".into(),
                translate_outside_set: false,
                show_original: false,
            })
    }

    /// Whether the server is reachable and the session is receiving events.
    pub fn is_online(&self) -> bool {
        self.shared.online.load(Ordering::Relaxed)
    }

    /// Whether events arrive over a WebSocket (otherwise by long polling).
    pub fn uses_websocket(&self) -> bool {
        self.shared.websocket.load(Ordering::Relaxed)
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

    /// Sends a message. `@name` in the text notifies that member.
    pub async fn send(&self, channel_id: &str, text: &str) -> Result<ChatMessage> {
        self.post(channel_id, text, None).await
    }

    /// Answers a message in the same channel; its author is notified.
    pub async fn reply(
        &self,
        channel_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<ChatMessage> {
        self.post(channel_id, text, Some(message_id)).await
    }

    async fn post(
        &self,
        channel_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<ChatMessage> {
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
                    reply_to: reply_to.map(str::to_string),
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
        let removed = self
            .shared
            .state
            .lock()
            .remove_message(channel_id, message_id);
        if removed.is_some() {
            self.shared.emit(Update::Message {
                guild_id,
                channel_id: channel_id.to_string(),
                message_id: message_id.to_string(),
                change: MessageChange::Deleted,
            });
        }
        Ok(())
    }

    // ---- Reading --------------------------------------------------------

    pub fn unread(&self, channel_id: &str) -> Unread {
        self.shared.state.lock().unread(channel_id)
    }

    /// Unread channels of a guild, together: unread if any is, with the
    /// mentions of all of them.
    pub fn guild_unread(&self, guild_id: &str) -> Unread {
        let state = self.shared.state.lock();
        let Some(view) = state.guild(guild_id) else {
            return Unread::default();
        };
        view.channels
            .iter()
            .map(|channel| state.unread(&channel.id))
            .fold(Unread::default(), |total, one| Unread {
                unread: total.unread || one.unread,
                mentions: total.mentions + one.mentions,
            })
    }

    /// Marks everything in the channel read, here and on the user's other
    /// devices.
    pub async fn mark_read(&self, channel_id: &str) -> Result<()> {
        self.shared.mark_read(channel_id).await
    }

    /// The channel the user is looking at, if any. Its messages do not
    /// notify and are marked read as they arrive; focusing marks it read.
    pub fn focus(&self, channel_id: Option<&str>) {
        *self.shared.focus.lock() = channel_id.map(str::to_string);
        if let Some(channel_id) = channel_id {
            self.shared.read_soon(channel_id);
        }
    }

    pub fn notification_settings(&self, guild_id: &str) -> Option<NotificationSettings> {
        self.guild(guild_id).map(|view| view.notifications)
    }

    /// Saves what the user wants to hear about in a guild; their other
    /// devices follow.
    pub async fn set_notifications(
        &self,
        settings: NotificationSettings,
    ) -> Result<NotificationSettings> {
        let settings = self
            .shared
            .client
            .guilds()
            .put_notification_settings(self.shared.client.jwt(), settings)
            .await?;
        let changed = {
            let mut state = self.shared.state.lock();
            let view = state
                .guilds
                .iter_mut()
                .find(|view| view.guild.id == settings.guild_id);
            match view {
                Some(view) if view.notifications != settings => {
                    view.notifications = settings.clone();
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.shared.emit(Update::Guild {
                guild_id: settings.guild_id.clone(),
                change: GuildChange::Notifications,
            });
        }
        Ok(settings)
    }

    // ---- Guilds ---------------------------------------------------------

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

    /// The owner cannot leave.
    pub async fn leave(&self, guild_id: &str) -> Result<()> {
        self.shared
            .client
            .guilds()
            .leave_guild(self.shared.client.jwt(), guild_id.to_string())
            .await?;
        if self.shared.state.lock().remove_guild(guild_id) {
            self.shared.emit(Update::Guild {
                guild_id: guild_id.to_string(),
                change: GuildChange::Left,
            });
        }
        Ok(())
    }

    /// Owner only. With `ban`, the user cannot come back until unbanned.
    pub async fn remove_member(&self, guild_id: &str, user_id: &str, ban: bool) -> Result<()> {
        Ok(self
            .shared
            .client
            .guilds()
            .remove_member(
                self.shared.client.jwt(),
                guild_id.to_string(),
                user_id.to_string(),
                ban,
            )
            .await?)
    }

    pub async fn unban(&self, guild_id: &str, user_id: &str) -> Result<()> {
        Ok(self
            .shared
            .client
            .guilds()
            .unban_member(
                self.shared.client.jwt(),
                guild_id.to_string(),
                user_id.to_string(),
            )
            .await?)
    }

    pub async fn bans(&self, guild_id: &str) -> Result<Vec<Ban>> {
        Ok(self
            .shared
            .client
            .guilds()
            .get_bans(self.shared.client.jwt(), guild_id.to_string())
            .await?)
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

    /// Valid invites: all of them for the owner, the user's own otherwise.
    pub async fn invites(&self, guild_id: &str) -> Result<Vec<Invite>> {
        Ok(self
            .shared
            .client
            .guilds()
            .get_invites(self.shared.client.jwt(), guild_id.to_string())
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

    /// Models the guild may translate with; the first is the default. Empty
    /// for guilds that translate on device only.
    pub async fn models(&self, guild_id: &str) -> Result<Vec<TranslationModel>> {
        Ok(self
            .shared
            .client
            .guilds()
            .get_models(self.shared.client.jwt(), guild_id.to_string())
            .await?)
    }

    /// Owner only. Chooses what the LLM translates the guild with; `None`
    /// is the default model. The languages stay as they are.
    pub async fn set_model(&self, guild_id: &str, model: Option<String>) -> Result<LanguagePolicy> {
        let policy = self
            .guild(guild_id)
            .map(|view| view.policy)
            .ok_or(Error::Api(ApiError::NotFound))?;
        self.set_languages(LanguagePolicyUpdate {
            guild_id: guild_id.to_string(),
            languages: policy.languages,
            llm_languages: policy.llm_languages,
            region_mode: policy.region_mode,
            aliases: policy.aliases,
            translate_regional_variants: policy.translate_regional_variants,
            model,
        })
        .await
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

    /// Writes the cache now (it is also written every few seconds while
    /// things change, and when the session is dropped).
    pub fn save_cache(&self) {
        self.shared.save_cache();
    }
}

impl Shared {
    fn emit(&self, update: Update) {
        self.changes.fetch_add(1, Ordering::Relaxed);
        let _ = self.updates.send(update);
    }

    fn guild_of(&self, channel_id: &str) -> Result<String> {
        self.state
            .lock()
            .channel(channel_id)
            .map(|channel| channel.guild_id.clone())
            .ok_or(Error::Api(ApiError::NotFound))
    }

    async fn run(self: Arc<Self>, mut session: Option<(String, i64)>) {
        let mut backoff = self.config.retry_min;
        let mut offline = false;
        let mut websocket = self.config.transport != Transport::LongPoll;
        loop {
            let result = match &mut session {
                None => match self.reconnect().await {
                    Ok(ready) => {
                        session = Some(ready);
                        self.came_online(&mut offline, true);
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Some((id, cursor)) if websocket => {
                    let id = id.clone();
                    match self.stream(&id, cursor, &mut offline).await {
                        websocket::Ended::Unsupported
                            if self.config.transport == Transport::Auto =>
                        {
                            tracing::info!("no WebSocket gateway; long polling");
                            websocket = false;
                            continue;
                        }
                        websocket::Ended::Unsupported => Err(Error::Api(ApiError::NotFound)),
                        websocket::Ended::Failed(error) => Err(error),
                    }
                }
                Some((id, cursor)) => {
                    self.websocket.store(false, Ordering::Relaxed);
                    match self
                        .client
                        .gateway()
                        .poll(
                            self.client.jwt(),
                            id.clone(),
                            *cursor,
                            self.config.poll_wait.as_millis() as i64,
                        )
                        .await
                    {
                        Ok(batch) => {
                            self.came_online(&mut offline, false);
                            for event in batch.events {
                                self.apply(event);
                            }
                            *cursor = batch.next;
                            Ok(())
                        }
                        Err(error) => Err(Error::from(error)),
                    }
                }
            };
            match result {
                Ok(()) => backoff = self.config.retry_min,
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
                        self.online.store(false, Ordering::Relaxed);
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(self.config.retry_max);
                }
            }
        }
    }

    fn came_online(&self, offline: &mut bool, reloaded: bool) {
        self.online.store(true, Ordering::Relaxed);
        if *offline || reloaded {
            self.emit(Update::Online { reloaded });
        }
        *offline = false;
    }

    /// Receives events over the WebSocket until it ends.
    async fn stream(
        self: &Arc<Self>,
        id: &str,
        cursor: &mut i64,
        offline: &mut bool,
    ) -> websocket::Ended {
        let idle = self.config.poll_wait * 2 + Duration::from_secs(5);
        let after = *cursor;
        websocket::stream(
            &self.client,
            id,
            after,
            idle,
            || {
                self.websocket.store(true, Ordering::Relaxed);
                self.came_online(offline, false);
            },
            |batch| {
                for event in batch.events {
                    self.apply(event);
                }
                *cursor = batch.next;
            },
        )
        .await
    }

    fn apply(self: &Arc<Self>, event: babel_api_edge_client::types::Event) {
        let (updates, notify) = {
            let mut state = self.state.lock();
            let created = (event.kind == babel_api_edge_client::types::EventKind::MessageCreated)
                .then(|| event.message.clone())
                .flatten();
            let updates = state.apply(event);
            // Only messages new to this session notify.
            let notify = created
                .filter(|message| {
                    updates.iter().any(|update| {
                        matches!(update, Update::Message { message_id, change: MessageChange::Created, .. } if *message_id == message.id)
                    })
                })
                .and_then(|message| {
                    let focus = self.focus.lock().clone();
                    state
                        .notify_reason(&message, focus.as_deref())
                        .map(|reason| (message, reason))
                });
            (updates, notify)
        };
        let focus = self.focus.lock().clone();
        for update in updates {
            if let Update::Message {
                channel_id,
                message_id,
                change,
                ..
            } = &update
            {
                if matches!(
                    change,
                    MessageChange::Created | MessageChange::Edited | MessageChange::Translated
                ) {
                    self.prepare(channel_id, message_id);
                }
                if *change == MessageChange::Created && focus.as_deref() == Some(channel_id) {
                    self.read_soon(channel_id);
                }
            }
            self.emit(update);
        }
        if let Some((message, reason)) = notify {
            self.notify_when_ready(message, reason);
        }
    }

    /// Sends a notification once the message's display has settled (or
    /// `notify_wait` has passed), so it shows the reader's language.
    fn notify_when_ready(self: &Arc<Self>, message: Message, reason: NotifyReason) {
        let shared = self.clone();
        let mut updates = self.updates.subscribe();
        self.runtime.spawn(async move {
            let deadline = tokio::time::Instant::now() + shared.config.notify_wait;
            let chat = loop {
                let current = {
                    let state = shared.state.lock();
                    match state.entry(&message.channel_id, &message.id) {
                        Some(entry) => shared.chat_message(&state, entry),
                        // Deleted before anyone saw it.
                        None => return,
                    }
                };
                if current.display.is_settled() {
                    break current;
                }
                match tokio::time::timeout_at(deadline, updates.recv()).await {
                    Err(_) => break current,
                    Ok(Err(broadcast::error::RecvError::Closed)) => return,
                    Ok(_) => {}
                }
            };
            shared.emit(Update::Notification(Notification {
                guild_id: message.guild_id,
                reason,
                message: chat,
            }));
        });
    }

    /// Starts device translation early, before the app asks.
    fn prepare(self: &Arc<Self>, channel_id: &str, message_id: &str) {
        let state = self.state.lock();
        if let Some(entry) = state.entry(channel_id, message_id) {
            self.chat_message(&state, entry);
        }
    }

    /// Marks a channel read after `read_delay`, once for a burst.
    fn read_soon(self: &Arc<Self>, channel_id: &str) {
        if !self.reading.lock().insert(channel_id.to_string()) {
            return;
        }
        let shared = self.clone();
        let channel_id = channel_id.to_string();
        self.runtime.spawn(async move {
            tokio::time::sleep(shared.config.read_delay).await;
            shared.reading.lock().remove(&channel_id);
            if let Err(error) = shared.mark_read(&channel_id).await {
                tracing::debug!(%error, channel_id, "could not mark a channel read");
            }
        });
    }

    async fn mark_read(&self, channel_id: &str) -> Result<()> {
        let (guild_id, newest) = {
            let state = self.state.lock();
            let channel = state
                .channel(channel_id)
                .ok_or(Error::Api(ApiError::NotFound))?;
            if !state.unread(channel_id).unread && state.unread(channel_id).mentions == 0 {
                return Ok(());
            }
            let newest = channel
                .last_message_id
                .clone()
                .unwrap_or_else(|| "0".into());
            (channel.guild_id.clone(), newest)
        };
        let read = self
            .client
            .messages()
            .ack(
                self.client.jwt(),
                guild_id.clone(),
                channel_id.to_string(),
                newest,
            )
            .await?;
        self.put_read_state(&guild_id, read);
        Ok(())
    }

    fn put_read_state(&self, guild_id: &str, read: babel_api_edge_client::types::ReadState) {
        let channel_id = read.channel_id.clone();
        let changed = {
            let mut state = self.state.lock();
            let before = state.unread(&channel_id);
            state.put_read_state(guild_id, read);
            state.unread(&channel_id) != before
        };
        if changed {
            self.emit(Update::Unread {
                guild_id: guild_id.to_string(),
                channel_id,
            });
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
        let (change, unread_changed) = {
            let mut state = self.state.lock();
            let before = state.unread(&channel_id);
            let change = state.put_live(message);
            (change, state.unread(&channel_id) != before)
        };
        if let Some(change) = change {
            self.emit(Update::Message {
                guild_id: guild_id.clone(),
                channel_id: channel_id.clone(),
                message_id: message_id.clone(),
                change,
            });
        }
        if unread_changed {
            self.emit(Update::Unread {
                guild_id,
                channel_id: channel_id.clone(),
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

    /// Writes the cache every `cache_interval` while something changes.
    async fn keep_cache(self: Arc<Self>) {
        let mut saved = 0;
        loop {
            tokio::time::sleep(self.config.cache_interval).await;
            let changes = self.changes.load(Ordering::Relaxed);
            if changes != saved {
                saved = changes;
                let shared = self.clone();
                let _ = tokio::task::spawn_blocking(move || shared.save_cache()).await;
            }
        }
    }

    fn save_cache(&self) {
        let Some(path) = &self.config.cache else {
            return;
        };
        let result = cache::save(path, self.client.server(), &self.state.lock());
        if let Err(error) = result {
            tracing::warn!(%error, path = %path.display(), "could not write the cache");
        }
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
                    // Failures that may pass (a download, a crash) are tried
                    // again when the message is looked at later.
                    let retry = state
                        .device_failed
                        .get(&job)
                        .is_some_and(|at| at.elapsed() >= DEVICE_RETRY);
                    if result.is_none() || retry {
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
            };
            {
                let mut state = shared.state.lock();
                if state.device.len() >= DEVICE_CACHE_LIMIT {
                    state.device.clear();
                    state.device_failed.clear();
                }
                match &result {
                    Err(DeviceError::Failed(_)) => {
                        state.device_failed.insert(job.clone(), Instant::now());
                    }
                    _ => {
                        state.device_failed.remove(&job);
                    }
                }
                state
                    .device
                    .insert(job.clone(), result.map_err(|error| error.to_string()));
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
