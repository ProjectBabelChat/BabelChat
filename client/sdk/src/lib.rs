//! The BabelChat client SDK: everything an app needs except the UI.
//!
//! ```text
//!  app (CLI today, GPUI later)
//!    │  calls                     ▲ Update (what changed)
//!    ▼                            │
//!  Session ── state: guilds, channels, messages, translations
//!    │        display: which text each message shows this reader
//!    │        device:  on-device translation, through English
//!    ▼
//!  Edge (HTTP: requests + long-poll gateway)
//! ```
//!
//! An app signs in with a [`Client`], [`Client::connect`]s a [`Session`],
//! then reads state from the session and redraws when an [`Update`] says
//! something changed. The session keeps itself connected: it retries when
//! the network drops and reloads when the Edge lost its place.
//!
//! Every message comes with a [`Display`]: the text this reader should see
//! and where it came from (the original, the server's LLM, or the device).
//! See docs/architecture/client.md.
//!
//! The SDK runs on Tokio. Apps on another executor (GPUI) start a Tokio
//! runtime on a background thread and connect from inside it. Synchronous
//! [`Session`] methods can then be called from any thread; async ones are
//! spawned on that runtime and their `JoinHandle` awaited from anywhere.

mod api;
mod cache;
mod device;
mod display;
mod session;
mod state;
mod websocket;

pub use api::{Client, Error, Result};
pub use babel_api_edge_client::types::{
    ApiError, Ban, Channel, ChannelKind, ChannelNotify, GuildView, Invite, LanguageAlias,
    LanguagePolicy, LanguagePolicyUpdate, LanguageView, Member, Message, MessageTranslation,
    NotificationSettings, NotifyLevel, ReadState, RegionMode, Token, Translation, UserSettings,
};
pub use device::{DeviceError, DeviceTranslator, MockDeviceTranslator};
pub use display::{Display, Shown};
pub use session::{
    ChatMessage, GuildChange, MessageChange, Notification, NotifyReason, Session, SessionConfig,
    Transport, Unread, Update,
};

// Apps share one session between their UI thread and the runtime.
const _: () = {
    const fn shareable<T: Send + Sync>() {}
    shareable::<Session>();
    shareable::<Client>();
};
