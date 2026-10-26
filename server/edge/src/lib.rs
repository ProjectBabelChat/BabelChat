//! The Edge: the only public entry point.
//!
//! ```text
//! client ─HTTPS─▶ Edge ─┬─▶ Account          (login; tokens are verified here, locally)
//!                       ├─▶ Directory        (placement: guild -> cells)
//!                       ├─▶ Message Cell     (guild data, permissions)
//!                       └─▶ Translation Cell (policy, translations)
//! ```
//!
//! The Edge authenticates, rate-limits, resolves placement, and joins the
//! answers of several services into what the client needs (a message with
//! its translation in the reader's language). It keeps no state that cannot
//! be rebuilt, so any number of Edges can run side by side.
//!
//! See docs/architecture/edge.md.

mod convert;
mod gateway;
mod placement;
mod websocket;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use babel_api_account::types::{Account, Credentials, Registration};
use babel_api_directory::types::{Directory, GuildRequest, Placement};
use babel_api_edge::types::{self as public, ApiError, Claim, EventKind};
use babel_api_message_cell::types::{self as cell, MessageCell, MessageCellError, Permission};
use babel_api_translation_cell::types::{
    self as translation, MessageRef, PolicyUpdate, RegionMode, SpacePolicy, TranslationCell,
    TranslationEventKind, TranslationQuery,
};
use babel_common::{now_ms, poll_timeout, random_id};
use babel_lang::Normalizer;
use gateway::{Sessions, event};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use parking_lot::{Mutex, RwLock};
pub use placement::Placements;

/// The services an Edge talks to. In the all-in-one server they are
/// in-process; split deployments put HTTP clients behind the same traits.
pub struct Cluster {
    pub account: Arc<dyn Account>,
    pub directory: Arc<dyn Directory>,
    pub message_cells: HashMap<String, Arc<dyn MessageCell>>,
    pub translation_cells: HashMap<String, Arc<dyn TranslationCell>>,
}

pub struct EdgeConfig {
    /// Development only: production Edges verify with the Account service's
    /// public key.
    pub jwt_secret: Vec<u8>,
    /// Sustained writes (messages, guilds, channels) per user per second.
    pub writes_per_second: f64,
    pub write_burst: f64,
}

#[derive(Clone)]
pub struct Edge {
    inner: Arc<Inner>,
}

struct Inner {
    cluster: Cluster,
    placements: Arc<Placements>,
    normalizer: Normalizer,
    decoding_key: DecodingKey,
    validation: Validation,
    limiter: RateLimiter,
    settings: Mutex<HashMap<String, (public::UserSettings, Instant)>>,
    policies: RwLock<HashMap<String, SpacePolicy>>,
    sessions: Sessions,
    followed: Mutex<HashSet<String>>,
}

const SETTINGS_TTL: Duration = Duration::from_secs(60);
const FOLLOW_WAIT_MS: i64 = 25_000;
const RETRY: Duration = Duration::from_secs(1);
const MAX_PAGE: i64 = 100;
const DEFAULT_INVITE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

type ApiResult<T> = Result<T, ApiError>;

impl Edge {
    /// Starts an Edge: its placement watch and session sweeper. Must be
    /// called within a Tokio runtime.
    pub fn start(cluster: Cluster, config: EdgeConfig) -> Self {
        let placements = Placements::start(cluster.directory.clone());
        let inner = Arc::new(Inner {
            cluster,
            placements,
            normalizer: Normalizer::new(),
            decoding_key: DecodingKey::from_secret(&config.jwt_secret),
            validation: Validation::new(Algorithm::HS256),
            limiter: RateLimiter {
                per_second: config.writes_per_second,
                burst: config.write_burst,
                buckets: Mutex::default(),
            },
            settings: Mutex::default(),
            policies: RwLock::default(),
            sessions: Sessions::default(),
            followed: Mutex::default(),
        });

        let sweeper = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(inner) = sweeper.upgrade() {
                inner.sessions.sweep();
                drop(inner);
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });

        Self { inner }
    }

    /// Routes served next to the generated API: the WebSocket gateway
    /// (`GET /gateway/ws`).
    pub fn websocket_router(&self) -> axum::Router {
        websocket::router(self.clone())
    }
}

struct RateLimiter {
    per_second: f64,
    burst: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimiter {
    fn allow(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock();
        let (tokens, last) = buckets
            .entry(key.to_string())
            .or_insert((self.burst, Instant::now()));
        *tokens = (*tokens + last.elapsed().as_secs_f64() * self.per_second).min(self.burst);
        *last = Instant::now();
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Where a guild lives, resolved for one request.
struct Route {
    placement: Placement,
    cell: Arc<dyn MessageCell>,
    translation: Arc<dyn TranslationCell>,
}

impl Inner {
    fn write_allowed(&self, claim: &Claim) -> ApiResult<()> {
        if self.limiter.allow(&claim.sub) {
            Ok(())
        } else {
            Err(ApiError::RateLimited)
        }
    }

    async fn route(&self, guild: &str) -> ApiResult<Route> {
        let placement = self
            .placements
            .resolve(guild)
            .await
            .map_err(convert::directory_error)?;
        let cell = self
            .cluster
            .message_cells
            .get(&placement.message_cell)
            .cloned()
            .ok_or(ApiError::Unavailable)?;
        let translation = self
            .cluster
            .translation_cells
            .get(&placement.translation_cell)
            .cloned()
            .ok_or(ApiError::Unavailable)?;
        Ok(Route {
            placement,
            cell,
            translation,
        })
    }

    async fn settings(&self, user: &str) -> ApiResult<public::UserSettings> {
        if let Some((settings, at)) = self.settings.lock().get(user)
            && at.elapsed() < SETTINGS_TTL
        {
            return Ok(settings.clone());
        }
        let settings = self
            .cluster
            .account
            .get_settings(user.to_string())
            .await
            .map_err(convert::account_error)?;
        let settings = public::UserSettings {
            locale: settings.locale,
            translate_outside_set: settings.translate_outside_set,
            show_original: settings.show_original,
        };
        self.settings
            .lock()
            .insert(user.to_string(), (settings.clone(), Instant::now()));
        Ok(settings)
    }

    async fn policy(&self, route: &Route) -> ApiResult<SpacePolicy> {
        let guild = &route.placement.guild_id;
        if let Some(policy) = self.policies.read().get(guild) {
            return Ok(policy.clone());
        }
        let policy = route
            .translation
            .get_policy(guild.clone())
            .await
            .map_err(convert::translation_error)?;
        self.remember_policy(policy.clone());
        Ok(policy)
    }

    fn remember_policy(&self, policy: SpacePolicy) {
        let mut policies = self.policies.write();
        let newer = policies
            .get(&policy.space)
            .is_none_or(|known| known.version <= policy.version);
        if newer {
            policies.insert(policy.space.clone(), policy);
        }
    }

    /// How a user reads a guild under its current policy.
    fn view(&self, policy: &SpacePolicy, settings: &public::UserSettings) -> public::LanguageView {
        let resolved = babel_lang::resolve_viewer(
            &self.normalizer,
            &settings.locale,
            &babel_lang::RegionPolicy {
                mode: match policy.region_mode {
                    RegionMode::Merged => babel_lang::RegionMode::Merged,
                    RegionMode::Distinct => babel_lang::RegionMode::Distinct,
                },
                aliases: policy
                    .aliases
                    .iter()
                    .map(|alias| babel_lang::Alias {
                        from: alias.from.clone(),
                        to: alias.to.clone(),
                    })
                    .collect(),
            },
            &policy.languages,
            &policy.llm_languages,
            settings.translate_outside_set,
        );
        match resolved {
            Ok(viewer) => public::LanguageView {
                effective: viewer.effective,
                in_set: viewer.in_set,
                llm: viewer.llm,
                client_translation: viewer.client_translation,
            },
            Err(_) => public::LanguageView {
                effective: settings.locale.clone(),
                in_set: false,
                llm: false,
                client_translation: false,
            },
        }
    }

    /// Translations a user needs: their own language, plus the space's
    /// English as the pivot for on-device translation.
    fn wanted_langs(&self, policy: &SpacePolicy, view: &public::LanguageView) -> Vec<String> {
        let mut langs = vec![view.effective.clone()];
        if let Some(english) = policy
            .llm_languages
            .iter()
            .find(|tag| self.normalizer.is_english(tag))
            && *english != view.effective
        {
            langs.push(english.clone());
        }
        langs
    }

    fn guild_view(
        &self,
        snapshot: &cell::GuildSnapshot,
        policy: &SpacePolicy,
        settings: &public::UserSettings,
    ) -> public::GuildView {
        public::GuildView {
            guild: convert::guild(&snapshot.guild),
            channels: snapshot.channels.iter().map(convert::channel).collect(),
            members: snapshot.members.iter().map(convert::member).collect(),
            policy: convert::policy(policy),
            language: self.view(policy, settings),
            read_states: snapshot
                .read_states
                .iter()
                .map(convert::read_state)
                .collect(),
            notifications: convert::notifications(&snapshot.notifications),
        }
    }

    /// Makes `user`'s sessions (or one of them) receive `route`'s events.
    /// Must run before taking the snapshot the client starts from, so no
    /// event falls between the snapshot and the stream.
    async fn subscribe(
        self: &Arc<Self>,
        user: &str,
        session: Option<&str>,
        route: &Route,
    ) -> ApiResult<cell::GuildSnapshot> {
        let guild = &route.placement.guild_id;
        self.sessions.add_guild(user, session, guild);
        let snapshot = match route.cell.get_guild(guild.clone(), user.to_string()).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // Not a member (any more): the Directory's copy of the
                // membership lags behind the cell.
                self.sessions.cancel_guild(user, guild);
                return Err(convert::cell_error(error));
            }
        };
        self.sessions.confirm_guild(user, guild, snapshot.last_seq);
        self.follow(guild, snapshot.last_seq, &route.translation)
            .await;
        Ok(snapshot)
    }

    /// Starts following a guild's two logs, once per Edge.
    async fn follow(
        self: &Arc<Self>,
        guild: &str,
        guild_head: i64,
        translation: &Arc<dyn TranslationCell>,
    ) {
        if !self.followed.lock().insert(guild.to_string()) {
            return;
        }
        let space_head = translation
            .poll_events(guild.to_string(), i64::MAX, 0)
            .await
            .map(|batch| batch.head)
            .unwrap_or(0);
        tokio::spawn(self.clone().follow_guild(guild.to_string(), guild_head));
        tokio::spawn(self.clone().follow_space(guild.to_string(), space_head));
    }

    async fn follow_guild(self: Arc<Self>, guild: String, mut cursor: i64) {
        loop {
            let route = match self.route(&guild).await {
                Ok(route) => route,
                Err(ApiError::NotFound) => return,
                Err(_) => {
                    tokio::time::sleep(RETRY).await;
                    continue;
                }
            };
            match route
                .cell
                .poll_events(guild.clone(), cursor, FOLLOW_WAIT_MS)
                .await
            {
                Ok(batch) => {
                    for guild_event in batch.events {
                        self.on_guild_event(&guild, guild_event);
                    }
                    cursor = batch.next;
                }
                Err(MessageCellError::NotFound) => return,
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    }

    fn on_guild_event(&self, guild: &str, guild_event: cell::GuildEvent) {
        let public_event = match guild_event.kind {
            cell::GuildEventKind::GuildCreated => return,
            cell::GuildEventKind::MemberJoined => public::Event {
                member: guild_event.member.as_ref().map(convert::member),
                ..event(EventKind::MemberJoined, guild)
            },
            cell::GuildEventKind::MemberLeft | cell::GuildEventKind::MemberRemoved => {
                let Some(member) = &guild_event.member else {
                    return;
                };
                let kind = match guild_event.kind {
                    cell::GuildEventKind::MemberLeft => EventKind::MemberLeft,
                    _ => EventKind::MemberRemoved,
                };
                // The member's own sessions hear it too, as their last event
                // from this guild, unless they joined again since.
                let public_event = public::Event {
                    member: Some(convert::member(member)),
                    ..event(kind, guild)
                };
                self.sessions.broadcast(guild, |session| {
                    let rejoined = session.user_id == member.user_id
                        && session
                            .guilds
                            .get(guild)
                            .is_some_and(|from| *from >= guild_event.seq);
                    (!rejoined).then(|| public_event.clone())
                });
                self.sessions
                    .remove_guild(&member.user_id, guild, guild_event.seq);
                return;
            }
            cell::GuildEventKind::ChannelCreated => public::Event {
                channel: guild_event.channel.as_ref().map(convert::channel),
                ..event(EventKind::ChannelCreated, guild)
            },
            cell::GuildEventKind::MessageCreated => public::Event {
                message: guild_event.message.as_ref().map(convert::message),
                ..event(EventKind::MessageCreated, guild)
            },
            cell::GuildEventKind::MessageEdited => public::Event {
                message: guild_event.message.as_ref().map(convert::message),
                ..event(EventKind::MessageEdited, guild)
            },
            cell::GuildEventKind::MessageDeleted => public::Event {
                deleted: guild_event
                    .deleted
                    .as_ref()
                    .map(|deleted| public::DeletedMessage {
                        channel_id: deleted.channel_id.clone(),
                        message_id: deleted.message_id.clone(),
                    }),
                ..event(EventKind::MessageDeleted, guild)
            },
        };
        self.sessions
            .broadcast(guild, |_| Some(public_event.clone()));
    }

    async fn follow_space(self: Arc<Self>, guild: String, mut cursor: i64) {
        loop {
            let route = match self.route(&guild).await {
                Ok(route) => route,
                Err(ApiError::NotFound) => return,
                Err(_) => {
                    tokio::time::sleep(RETRY).await;
                    continue;
                }
            };
            match route
                .translation
                .poll_events(guild.clone(), cursor, FOLLOW_WAIT_MS)
                .await
            {
                Ok(batch) => {
                    for space_event in batch.events {
                        self.on_space_event(&route, space_event).await;
                    }
                    cursor = batch.next;
                }
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    }

    async fn on_space_event(&self, route: &Route, space_event: translation::TranslationEvent) {
        let guild = &route.placement.guild_id;
        match space_event.kind {
            TranslationEventKind::Translated => {
                let (Some(translated), Ok(policy)) =
                    (space_event.translated, self.policy(route).await)
                else {
                    return;
                };
                self.sessions.broadcast(guild, |session| {
                    let view = self.view(&policy, &session.settings);
                    let langs = self.wanted_langs(&policy, &view);
                    Some(public::Event {
                        translation: Some(convert::translation(&translated, &langs)),
                        ..event(EventKind::MessageTranslated, guild)
                    })
                });
            }
            TranslationEventKind::Policy => {
                let Some(policy) = space_event.policy else {
                    return;
                };
                self.remember_policy(policy.clone());
                self.sessions.broadcast(guild, |session| {
                    Some(public::Event {
                        policy: Some(convert::policy(&policy)),
                        language: Some(self.view(&policy, &session.settings)),
                        ..event(EventKind::PolicyUpdated, guild)
                    })
                });
            }
            // Deletions already reach clients from the Message Cell.
            TranslationEventKind::Removed => {}
        }
    }

    /// Messages joined with their translations for one reader.
    async fn message_views(
        &self,
        route: &Route,
        messages: Vec<cell::Message>,
        settings: &public::UserSettings,
    ) -> ApiResult<Vec<public::MessageView>> {
        let policy = self.policy(route).await?;
        let view = self.view(&policy, settings);
        let langs = self.wanted_langs(&policy, &view);

        let refs = messages
            .iter()
            .map(|message| MessageRef {
                space: message.guild_id.clone(),
                conversation: message.channel_id.clone(),
                id: message.id.clone(),
            })
            .collect::<Vec<_>>();
        let mut translations = route
            .translation
            .get_translations(TranslationQuery {
                messages: refs,
                langs: langs.clone(),
            })
            .await
            .map_err(convert::translation_error)?
            .into_iter()
            .map(|translated| (translated.message.id.clone(), translated))
            .collect::<HashMap<_, _>>();

        Ok(messages
            .iter()
            .map(|message| public::MessageView {
                message: convert::message(message),
                translation: translations
                    .remove(&message.id)
                    .map(|translated| convert::translation(&translated, &langs)),
            })
            .collect())
    }

    /// Announces a guild to the user's other sessions.
    fn announce_guild(&self, user: &str, view: &public::GuildView) {
        self.sessions.send_to_user(
            user,
            public::Event {
                guild: Some(view.clone()),
                ..event(EventKind::GuildJoined, &view.guild.id)
            },
        );
    }
}

impl babel_api_edge::types::JwtValidator for Edge {
    fn validate<T: serde::de::DeserializeOwned>(&self, token: &str) -> Result<T, ()> {
        jsonwebtoken::decode::<T>(token, &self.inner.decoding_key, &self.inner.validation)
            .map(|data| data.claims)
            .map_err(|_| ())
    }
}

#[async_trait]
impl public::Auth for Edge {
    async fn register(&self, body: public::Registration) -> ApiResult<public::Token> {
        let token = self
            .inner
            .cluster
            .account
            .register(Registration {
                name: body.name,
                password: body.password,
                locale: body.locale,
            })
            .await
            .map_err(convert::account_error)?;
        Ok(public::Token {
            jwt: token.jwt,
            user_id: token.user_id,
            name: token.name,
            expires_at: token.expires_at,
        })
    }

    async fn login(&self, body: public::Credentials) -> ApiResult<public::Token> {
        let token = self
            .inner
            .cluster
            .account
            .login(Credentials {
                name: body.name,
                password: body.password,
            })
            .await
            .map_err(convert::account_error)?;
        Ok(public::Token {
            jwt: token.jwt,
            user_id: token.user_id,
            name: token.name,
            expires_at: token.expires_at,
        })
    }
}

#[async_trait]
impl public::Me for Edge {
    async fn get_settings(&self, claim: Claim) -> ApiResult<public::UserSettings> {
        self.inner.settings(&claim.sub).await
    }

    async fn put_settings(
        &self,
        claim: Claim,
        body: public::UserSettings,
    ) -> ApiResult<public::UserSettings> {
        self.inner
            .cluster
            .account
            .put_settings(
                babel_api_account::types::UserSettings {
                    locale: body.locale.clone(),
                    translate_outside_set: body.translate_outside_set,
                    show_original: body.show_original,
                },
                claim.sub.clone(),
            )
            .await
            .map_err(convert::account_error)?;
        self.inner
            .settings
            .lock()
            .insert(claim.sub.clone(), (body.clone(), Instant::now()));
        self.inner.sessions.update_settings(&claim.sub, &body);
        Ok(body)
    }
}

#[async_trait]
impl public::Guilds for Edge {
    async fn create_guild(
        &self,
        claim: Claim,
        body: public::NewGuild,
    ) -> ApiResult<public::GuildView> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let settings = inner.settings(&claim.sub).await?;

        let placement = inner
            .cluster
            .directory
            .create_guild(GuildRequest {
                owner_id: claim.sub.clone(),
                owner_name: claim.name.clone(),
                name: body.name,
                description: body.description,
            })
            .await
            .map_err(convert::directory_error)?;
        // This Edge knows at once; others learn through the Directory watch.
        inner.placements.insert(placement.clone());
        let route = inner.route(&placement.guild_id).await?;

        // Start with the owner's language, translated by the LLM if the
        // plan allows it.
        let default = route
            .translation
            .get_policy(placement.guild_id.clone())
            .await
            .map_err(convert::translation_error)?;
        let languages = vec![settings.locale.clone()];
        let policy = route
            .translation
            .put_policy(PolicyUpdate {
                space: placement.guild_id.clone(),
                llm_languages: if default.max_llm_languages > 0 {
                    languages.clone()
                } else {
                    Vec::new()
                },
                languages,
                region_mode: RegionMode::Merged,
                aliases: Vec::new(),
                translate_regional_variants: false,
            })
            .await
            .map_err(convert::translation_error)?;
        inner.remember_policy(policy.clone());

        let snapshot = inner.subscribe(&claim.sub, None, &route).await?;
        let view = inner.guild_view(&snapshot, &policy, &settings);
        inner.announce_guild(&claim.sub, &view);
        Ok(view)
    }

    async fn create_invite(
        &self,
        claim: Claim,
        body: public::NewInvite,
    ) -> ApiResult<public::Invite> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let expires_at = match body.max_age_secs {
            None => Some(now_ms() + DEFAULT_INVITE_AGE.as_millis() as i64),
            Some(0) => None,
            Some(secs) if secs > 0 => Some(now_ms().saturating_add(secs.saturating_mul(1000))),
            Some(_) => return Err(ApiError::InvalidRequest),
        };
        let route = inner.route(&body.guild_id).await?;
        let invite = route
            .cell
            .create_invite(cell::NewInvite {
                guild_id: body.guild_id,
                user_id: claim.sub,
                expires_at,
                max_uses: body.max_uses,
            })
            .await
            .map_err(convert::cell_error)?;
        Ok(convert::invite(&invite))
    }

    async fn revoke_invite(&self, claim: Claim, code: String) -> ApiResult<()> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let (guild_id, secret) = convert::parse_invite(&code).ok_or(ApiError::NotFound)?;
        let route = inner.route(guild_id).await?;
        route
            .cell
            .revoke_invite(guild_id.to_string(), secret.to_string(), claim.sub)
            .await
            .map_err(convert::cell_error)
    }

    async fn join_guild(&self, claim: Claim, invite: String) -> ApiResult<public::GuildView> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let settings = inner.settings(&claim.sub).await?;
        let (guild_id, secret) = convert::parse_invite(&invite).ok_or(ApiError::NotFound)?;
        let route = inner.route(guild_id).await?;
        route
            .cell
            .join_guild(
                guild_id.to_string(),
                secret.to_string(),
                claim.sub.clone(),
                claim.name.clone(),
            )
            .await
            .map_err(convert::cell_error)?;

        let snapshot = inner.subscribe(&claim.sub, None, &route).await?;
        let policy = inner.policy(&route).await?;
        let view = inner.guild_view(&snapshot, &policy, &settings);
        inner.announce_guild(&claim.sub, &view);
        Ok(view)
    }

    async fn get_guild(&self, claim: Claim, guild_id: String) -> ApiResult<public::GuildView> {
        let inner = &self.inner;
        let settings = inner.settings(&claim.sub).await?;
        let route = inner.route(&guild_id).await?;
        let snapshot = route
            .cell
            .get_guild(guild_id, claim.sub)
            .await
            .map_err(convert::cell_error)?;
        let policy = inner.policy(&route).await?;
        Ok(inner.guild_view(&snapshot, &policy, &settings))
    }

    async fn create_channel(
        &self,
        claim: Claim,
        body: public::NewChannel,
    ) -> ApiResult<public::Channel> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&body.guild_id).await?;
        let channel = route
            .cell
            .create_channel(cell::NewChannel {
                guild_id: body.guild_id,
                user_id: claim.sub,
                kind: convert::internal_channel_kind(body.kind),
                name: body.name,
                topic: body.topic,
                parent_id: body.parent_id,
                starter_message_id: body.starter_message_id,
            })
            .await
            .map_err(convert::cell_error)?;
        Ok(convert::channel(&channel))
    }

    async fn put_language_policy(
        &self,
        claim: Claim,
        body: public::LanguagePolicyUpdate,
    ) -> ApiResult<public::LanguagePolicy> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&body.guild_id).await?;
        // Roles live in the Message Cell; the policy in the Translation Cell.
        route
            .cell
            .check_permission(body.guild_id.clone(), claim.sub, Permission::ManageGuild)
            .await
            .map_err(convert::cell_error)?;
        let policy = route
            .translation
            .put_policy(convert::policy_update(body))
            .await
            .map_err(convert::translation_error)?;
        inner.remember_policy(policy.clone());
        Ok(convert::policy(&policy))
    }

    async fn get_invites(&self, claim: Claim, guild_id: String) -> ApiResult<Vec<public::Invite>> {
        let route = self.inner.route(&guild_id).await?;
        let invites = route
            .cell
            .get_invites(guild_id, claim.sub)
            .await
            .map_err(convert::cell_error)?;
        Ok(invites.iter().map(convert::invite).collect())
    }

    async fn leave_guild(&self, claim: Claim, guild_id: String) -> ApiResult<()> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&guild_id).await?;
        // Sessions stop receiving the guild when its log says so.
        route
            .cell
            .leave_guild(guild_id, claim.sub)
            .await
            .map_err(convert::cell_error)
    }

    async fn remove_member(
        &self,
        claim: Claim,
        guild_id: String,
        user_id: String,
        ban: bool,
    ) -> ApiResult<()> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&guild_id).await?;
        route
            .cell
            .remove_member(guild_id, claim.sub, user_id, ban)
            .await
            .map_err(convert::cell_error)
    }

    async fn unban_member(&self, claim: Claim, guild_id: String, user_id: String) -> ApiResult<()> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&guild_id).await?;
        route
            .cell
            .unban_member(guild_id, claim.sub, user_id)
            .await
            .map_err(convert::cell_error)
    }

    async fn get_bans(&self, claim: Claim, guild_id: String) -> ApiResult<Vec<public::Ban>> {
        let route = self.inner.route(&guild_id).await?;
        let bans = route
            .cell
            .get_bans(guild_id, claim.sub)
            .await
            .map_err(convert::cell_error)?;
        Ok(bans.iter().map(convert::ban).collect())
    }

    async fn put_notification_settings(
        &self,
        claim: Claim,
        body: public::NotificationSettings,
    ) -> ApiResult<public::NotificationSettings> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&body.guild_id).await?;
        let settings = route
            .cell
            .put_notification_settings(convert::internal_notifications(body), claim.sub.clone())
            .await
            .map_err(convert::cell_error)?;
        let settings = convert::notifications(&settings);
        inner.sessions.send_to_user(
            &claim.sub,
            public::Event {
                notifications: Some(settings.clone()),
                ..event(EventKind::NotificationsUpdated, &settings.guild_id)
            },
        );
        Ok(settings)
    }
}

#[async_trait]
impl public::Messages for Edge {
    async fn send_message(
        &self,
        claim: Claim,
        body: public::NewMessage,
    ) -> ApiResult<public::MessageView> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let settings = inner.settings(&claim.sub).await?;
        let route = inner.route(&body.guild_id).await?;
        let message = route
            .cell
            .send_message(cell::NewMessage {
                guild_id: body.guild_id,
                channel_id: body.channel_id,
                user_id: claim.sub,
                author_locale: Some(settings.locale),
                text: body.text,
                reply_to: body.reply_to,
            })
            .await
            .map_err(convert::cell_error)?;
        Ok(public::MessageView {
            message: convert::message(&message),
            translation: None,
        })
    }

    async fn edit_message(
        &self,
        claim: Claim,
        body: public::MessageEdit,
    ) -> ApiResult<public::MessageView> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&body.guild_id).await?;
        let message = route
            .cell
            .edit_message(cell::MessageEdit {
                guild_id: body.guild_id,
                channel_id: body.channel_id,
                message_id: body.message_id,
                user_id: claim.sub,
                text: body.text,
            })
            .await
            .map_err(convert::cell_error)?;
        Ok(public::MessageView {
            message: convert::message(&message),
            translation: None,
        })
    }

    async fn delete_message(
        &self,
        claim: Claim,
        guild_id: String,
        channel_id: String,
        message_id: String,
    ) -> ApiResult<()> {
        let inner = &self.inner;
        inner.write_allowed(&claim)?;
        let route = inner.route(&guild_id).await?;
        route
            .cell
            .delete_message(guild_id, channel_id, message_id, claim.sub)
            .await
            .map_err(convert::cell_error)
    }

    async fn get_messages(
        &self,
        claim: Claim,
        guild_id: String,
        channel_id: String,
        before: Option<String>,
        limit: i64,
    ) -> ApiResult<Vec<public::MessageView>> {
        let inner = &self.inner;
        let settings = inner.settings(&claim.sub).await?;
        let route = inner.route(&guild_id).await?;
        let messages = route
            .cell
            .get_messages(
                guild_id,
                channel_id,
                claim.sub,
                before,
                limit.clamp(1, MAX_PAGE),
            )
            .await
            .map_err(convert::cell_error)?;
        inner.message_views(&route, messages, &settings).await
    }

    async fn ack(
        &self,
        claim: Claim,
        guild_id: String,
        channel_id: String,
        message_id: String,
    ) -> ApiResult<public::ReadState> {
        let inner = &self.inner;
        // Each ack recounts mentions; clients batch them.
        inner.write_allowed(&claim)?;
        let route = inner.route(&guild_id).await?;
        let state = route
            .cell
            .ack(guild_id.clone(), channel_id, claim.sub.clone(), message_id)
            .await
            .map_err(convert::cell_error)?;
        let state = convert::read_state(&state);
        inner.sessions.send_to_user(
            &claim.sub,
            public::Event {
                read_state: Some(state.clone()),
                ..event(EventKind::ReadStateUpdated, &guild_id)
            },
        );
        Ok(state)
    }
}

#[async_trait]
impl public::Gateway for Edge {
    async fn connect(&self, claim: Claim) -> ApiResult<public::Ready> {
        let inner = &self.inner;
        let settings = inner.settings(&claim.sub).await?;
        let session_id = random_id();
        inner
            .sessions
            .create(&session_id, &claim.sub, settings.clone());

        let mut guilds = Vec::new();
        for guild in inner
            .cluster
            .directory
            .list_memberships(claim.sub.clone())
            .await
        {
            let Ok(route) = inner.route(&guild).await else {
                continue;
            };
            // Membership is copied from the cell asynchronously; skip guilds
            // the cell no longer lets this user see.
            let Ok(snapshot) = inner.subscribe(&claim.sub, Some(&session_id), &route).await else {
                continue;
            };
            let Ok(policy) = inner.policy(&route).await else {
                continue;
            };
            guilds.push(inner.guild_view(&snapshot, &policy, &settings));
        }

        Ok(public::Ready {
            session_id,
            user_id: claim.sub,
            guilds,
            cursor: 0,
        })
    }

    async fn poll(
        &self,
        claim: Claim,
        session_id: String,
        after: i64,
        wait_ms: i64,
    ) -> ApiResult<public::EventBatch> {
        let (events, next) = self
            .inner
            .sessions
            .poll(&session_id, &claim.sub, after, poll_timeout(wait_ms))
            .await
            .ok_or(ApiError::NotFound)?;
        Ok(public::EventBatch { events, next })
    }
}
