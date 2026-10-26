//! A copy of the session's state on disk, so an app starts with what it
//! showed last time, even offline, and does not translate the same
//! messages on device again.
//!
//! The cache is a convenience, never the truth: after connecting, the
//! session reloads guilds and the newest page of every cached channel from
//! the server as it would after any reconnect. One JSON file per signed-in
//! user, written whole (temporary file + rename), readable only by its
//! owner.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use babel_api_edge_client::types::{GuildView, Message, MessageTranslation, UserSettings};
use serde::{Deserialize, Serialize};

use crate::{
    display::DeviceJob,
    state::{ChannelLog, Entry, MessageKey, State},
};

/// Bump when the layout changes; older files are ignored.
const VERSION: u32 = 1;
/// Newest messages kept per channel.
const MESSAGES_PER_CHANNEL: usize = 100;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    server: String,
    user_id: String,
    settings: Option<UserSettings>,
    guilds: Vec<GuildView>,
    channels: Vec<CachedChannel>,
    /// Device translations of the cached messages.
    device: Vec<(DeviceJob, String)>,
}

#[derive(Serialize, Deserialize)]
struct CachedChannel {
    channel_id: String,
    guild_id: String,
    messages: Vec<(Message, Option<MessageTranslation>)>,
}

/// Loads the state saved for this user on this server, if any.
pub(crate) fn load(path: &Path, server: &str, user_id: &str) -> Option<State> {
    let bytes = std::fs::read(path).ok()?;
    let snapshot = match serde_json::from_slice::<Snapshot>(&bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "ignoring an unreadable cache");
            return None;
        }
    };
    if snapshot.version != VERSION || snapshot.server != server || snapshot.user_id != user_id {
        return None;
    }
    let mut state = State::default();
    state.user_id = snapshot.user_id;
    state.settings = snapshot.settings;
    state.guilds = snapshot.guilds;
    for channel in snapshot.channels {
        let messages = channel
            .messages
            .into_iter()
            .map(|(message, translation)| {
                (
                    MessageKey::new(&message.id),
                    Entry {
                        message,
                        translation,
                    },
                )
            })
            .collect();
        state.channels.insert(
            channel.channel_id,
            ChannelLog {
                guild_id: channel.guild_id,
                messages,
                // Reloaded from the server once connected.
                loaded: true,
                complete: false,
            },
        );
    }
    state.device = snapshot
        .device
        .into_iter()
        .map(|(job, text)| (job, Ok(text)))
        .collect();
    Some(state)
}

/// Writes what is worth keeping of `state`.
pub(crate) fn save(path: &Path, server: &str, state: &State) -> std::io::Result<()> {
    let mut kept = std::collections::HashSet::new();
    let channels = state
        .channels
        .iter()
        .filter(|(_, log)| !log.messages.is_empty())
        .map(|(channel_id, log)| CachedChannel {
            channel_id: channel_id.clone(),
            guild_id: log.guild_id.clone(),
            messages: log
                .messages
                .values()
                .rev()
                .take(MESSAGES_PER_CHANNEL)
                .rev()
                .map(|entry| {
                    kept.insert(entry.message.id.clone());
                    (entry.message.clone(), entry.translation.clone())
                })
                .collect(),
        })
        .collect();
    let device = state
        .device
        .iter()
        .filter(|(job, _)| kept.contains(&job.message_id))
        .filter_map(|(job, result)| Some((job.clone(), result.clone().ok()?)))
        .collect();
    let snapshot = Snapshot {
        version: VERSION,
        server: server.to_string(),
        user_id: state.user_id.clone(),
        settings: state.settings.clone(),
        guilds: state.guilds.clone(),
        channels,
        device,
    };
    let json = serde_json::to_vec(&snapshot).map_err(std::io::Error::other)?;
    write_private(path, &json)
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}
