//! What a Message Cell stores, and under which keys.
//!
//! ```text
//! table     key                              value
//! guilds    guild                            GuildRecord
//! members   guild / user                     Member
//! channels  guild / channel                  Channel
//! messages  guild / channel / message id     Message      (ids are time-ordered)
//! invites   guild / secret                   Invite
//! bans      guild / user                     Ban
//! reads     guild / user / channel           ReadState    (members' read markers)
//! notify    guild / user                     NotificationSettings
//! uploads   guild / user / attachment id     Upload       (files not sent yet; a day)
//! emoji     guild / emoji id                 Emoji        (the guild's own)
//! log       guild / seq                      GuildEvent   (the guild's event log)
//! outbox    seq                              OutboxEntry  (cell-wide, in commit order)
//! ```
//!
//! Every key of a guild starts with the guild ID, so moving a guild to
//! another cell is a range export of each table.

use babel_api_message_cell::types::{Attachment, Guild};
use babel_store::{Key, Table};
use serde::{Deserialize, Serialize};

pub const GUILDS: Table = "guilds";
pub const MEMBERS: Table = "members";
pub const CHANNELS: Table = "channels";
pub const MESSAGES: Table = "messages";
pub const INVITES: Table = "invites";
pub const BANS: Table = "bans";
pub const READS: Table = "reads";
pub const NOTIFY: Table = "notify";
pub const UPLOADS: Table = "uploads";
pub const EMOJI: Table = "emoji";
pub const LOG: Table = "log";
pub const OUTBOX: Table = "outbox";

pub const TABLES: &[Table] = &[
    GUILDS, MEMBERS, CHANNELS, MESSAGES, INVITES, BANS, READS, NOTIFY, UPLOADS, EMOJI, LOG, OUTBOX,
];

/// The tables holding a guild's entries (every key starts with the guild),
/// in the order a move copies them: the guild record last, so the guild is
/// whole on the new cell before it exists there.
pub const GUILD_TABLES: &[Table] = &[
    MEMBERS, CHANNELS, MESSAGES, INVITES, BANS, READS, NOTIFY, UPLOADS, EMOJI, LOG, GUILDS,
];

pub fn guild_key(guild: &str) -> Vec<u8> {
    Key::new().str(guild).build()
}

pub fn member_key(guild: &str, user: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).build()
}

pub fn invite_key(guild: &str, secret: &str) -> Vec<u8> {
    Key::new().str(guild).str(secret).build()
}

/// A member's read states. (Bans and notification settings use the member
/// key.)
pub fn read_prefix(guild: &str, user: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).build()
}

pub fn read_key(guild: &str, user: &str, channel: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).str(channel).build()
}

pub fn channel_key(guild: &str, channel: &str) -> Vec<u8> {
    Key::new().str(guild).str(channel).build()
}

pub fn message_prefix(guild: &str, channel: &str) -> Vec<u8> {
    Key::new().str(guild).str(channel).build()
}

pub fn message_key(guild: &str, channel: &str, id: u64) -> Vec<u8> {
    Key::new().str(guild).str(channel).u64(id).build()
}

pub fn upload_prefix(guild: &str, user: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).build()
}

pub fn upload_key(guild: &str, user: &str, id: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).str(id).build()
}

pub fn emoji_key(guild: &str, id: &str) -> Vec<u8> {
    Key::new().str(guild).str(id).build()
}

pub fn log_key(guild: &str, seq: i64) -> Vec<u8> {
    Key::new().str(guild).u64(seq as u64).build()
}

pub fn outbox_key(seq: i64) -> Vec<u8> {
    Key::new().u64(seq as u64).build()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildRecord {
    pub guild: Guild,
    /// Where the outbox forwards this guild's messages and context.
    pub translation_cell: String,
    /// Last sequence number of the guild's event log.
    pub last_seq: i64,
    /// Last message ID handed out, to keep IDs increasing across restarts.
    pub last_message_id: u64,
    /// Log entries up to this sequence number were trimmed.
    #[serde(default)]
    pub trimmed: i64,
    /// Being copied to another cell: writes wait.
    #[serde(default)]
    pub frozen: bool,
    /// Lives on another cell now; only this record is left, so that calls
    /// about the guild answer `moved`.
    #[serde(default)]
    pub moved: bool,
    /// Deleted by its owner: only this record and the log are left, until
    /// the log is trimmed, so Edges read the deletion before the guild
    /// answers `not_found`.
    #[serde(default)]
    pub deleted: bool,
}

/// A file a member put on a Media service and has not sent yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub channel_id: String,
    pub attachment: Attachment,
    pub uploaded_at: i64,
}

/// A pointer to a log entry that other services have not seen yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub guild: String,
    pub seq: i64,
}
