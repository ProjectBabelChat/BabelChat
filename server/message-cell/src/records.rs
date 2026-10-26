//! What a Message Cell stores, and under which keys.
//!
//! ```text
//! table     key                              value
//! guilds    guild                            GuildRecord
//! members   guild / user                     Member
//! channels  guild / channel                  Channel
//! messages  guild / channel / message id     Message      (ids are time-ordered)
//! invites   guild / secret                   Invite
//! log       guild / seq                      GuildEvent   (the guild's event log)
//! outbox    seq                              OutboxEntry  (cell-wide, in commit order)
//! ```
//!
//! Every key of a guild starts with the guild ID, so moving a guild to
//! another cell is a range export of each table.

use babel_api_message_cell::types::Guild;
use babel_store::{Key, Table};
use serde::{Deserialize, Serialize};

pub const GUILDS: Table = "guilds";
pub const MEMBERS: Table = "members";
pub const CHANNELS: Table = "channels";
pub const MESSAGES: Table = "messages";
pub const INVITES: Table = "invites";
pub const LOG: Table = "log";
pub const OUTBOX: Table = "outbox";

pub const TABLES: &[Table] = &[GUILDS, MEMBERS, CHANNELS, MESSAGES, INVITES, LOG, OUTBOX];

pub fn guild_key(guild: &str) -> Vec<u8> {
    Key::new().str(guild).build()
}

pub fn member_key(guild: &str, user: &str) -> Vec<u8> {
    Key::new().str(guild).str(user).build()
}

pub fn invite_key(guild: &str, secret: &str) -> Vec<u8> {
    Key::new().str(guild).str(secret).build()
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
}

/// A pointer to a log entry that other services have not seen yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub guild: String,
    pub seq: i64,
}
