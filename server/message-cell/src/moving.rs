//! Moving a guild to another cell, driven by the Directory.
//!
//! ```text
//! Directory          old cell                       new cell
//!    │ discard ─────────────────────────────────────▶ (leftovers of a failed move go)
//!    │ freeze ────────▶ writes wait; outbox drains
//!    │ export ────────▶ pages of entries ─▶ import ─▶ stored, still frozen
//!    │ placement switches (Edges follow it)
//!    │ finish(moved: false) ────────────────────────▶ writes go on here
//!    │ finish(moved: true) ─▶ data dropped; waiting
//!    │                        writers hear `moved`
//! ```
//!
//! Entries move as they are stored, so the log keeps its sequence numbers
//! and Edges following it carry on from where they were. Only the guild's
//! record is left behind, answering `moved`.

use std::time::Duration;

use babel_api_message_cell::types::{GuildEntry, GuildExport, GuildImport, MessageCellError};
use babel_store::{Batch, Scan};

use crate::{
    CellResult, Inner,
    records::{GUILD_TABLES, GUILDS, GuildRecord, OUTBOX, OutboxEntry, guild_key},
    storage,
};

/// How long writers wait for a move before giving up.
pub(crate) const WRITE_WAIT: Duration = Duration::from_secs(30);
/// How long freezing waits for the outbox to deliver the guild's entries.
const DRAIN_WAIT: Duration = Duration::from_secs(10);
const MAX_PAGE: usize = 1000;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> CellResult<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err(MessageCellError::InvalidRequest);
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16))
        .collect::<Result<_, _>>()
        .map_err(|_| MessageCellError::InvalidRequest)
}

impl Inner {
    /// The guild's record, moved or not.
    fn raw_guild(&self, guild: &str) -> CellResult<GuildRecord> {
        self.store
            .get_value(GUILDS, &guild_key(guild))
            .map_err(storage)?
            .ok_or(MessageCellError::NotFound)
    }

    fn put_guild(&self, record: &GuildRecord) -> CellResult<()> {
        let mut batch = Batch::new();
        batch.put_value(GUILDS, guild_key(&record.guild.id), record);
        self.store.write(batch).map_err(storage)
    }

    fn pending(&self, guild: &str) -> CellResult<bool> {
        Ok(self
            .store
            .scan_values::<OutboxEntry>(Scan::prefix(OUTBOX, &[]))
            .map_err(storage)?
            .iter()
            .any(|(_, entry)| entry.guild == guild))
    }

    pub(crate) async fn freeze_guild(&self, guild: &str) -> CellResult<()> {
        {
            let _guard = self.lock(guild).await;
            let mut record = self.guild(guild)?;
            if !record.frozen {
                record.frozen = true;
                self.put_guild(&record)?;
            }
        }
        // What the outbox still has to forward must leave from here: the new
        // cell's outbox starts empty.
        let deadline = tokio::time::Instant::now() + DRAIN_WAIT;
        while self.pending(guild)? {
            if tokio::time::Instant::now() > deadline {
                self.finish_move(guild, false).await?;
                return Err(MessageCellError::Unavailable);
            }
            self.outbox_signal.notify_one();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    }

    /// Cursors are `<table index>:<hex key of the last entry>`.
    pub(crate) fn export_guild(
        &self,
        guild: &str,
        cursor: Option<String>,
        limit: i64,
    ) -> CellResult<GuildExport> {
        if !self.guild(guild)?.frozen {
            return Err(MessageCellError::InvalidRequest);
        }
        let (mut table, mut after) = match cursor {
            None => (0, None),
            Some(cursor) => {
                let (table, key) = cursor
                    .split_once(':')
                    .ok_or(MessageCellError::InvalidRequest)?;
                let table = table
                    .parse::<usize>()
                    .map_err(|_| MessageCellError::InvalidRequest)?;
                (table, Some(unhex(key)?).filter(|key| !key.is_empty()))
            }
        };
        let limit = (limit.max(1) as usize).min(MAX_PAGE);
        let prefix = guild_key(guild);
        let mut entries = Vec::new();
        while table < GUILD_TABLES.len() && entries.len() < limit {
            let wanted = limit - entries.len();
            let mut scan = Scan::prefix(GUILD_TABLES[table], &prefix).limit(wanted);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan(scan).map_err(storage)?;
            let more = page.len() == wanted;
            for (key, value) in page {
                entries.push(GuildEntry {
                    table: GUILD_TABLES[table].to_string(),
                    key: hex(&key),
                    value: hex(&value),
                });
                after = Some(key);
            }
            if !more {
                table += 1;
                after = None;
            }
        }
        Ok(GuildExport {
            entries,
            cursor: (table < GUILD_TABLES.len())
                .then(|| format!("{table}:{}", hex(after.as_deref().unwrap_or_default()))),
        })
    }

    pub(crate) fn import_guild(&self, import: GuildImport) -> CellResult<()> {
        let prefix = guild_key(&import.guild_id);
        let mut batch = Batch::new();
        let mut record = None;
        for entry in import.entries {
            let table = *GUILD_TABLES
                .iter()
                .find(|table| **table == entry.table)
                .ok_or(MessageCellError::InvalidRequest)?;
            let key = unhex(&entry.key)?;
            // Only this guild's entries.
            if !key.starts_with(&prefix) {
                return Err(MessageCellError::InvalidRequest);
            }
            let value = unhex(&entry.value)?;
            if table == GUILDS {
                let mut imported = babel_store::decode::<GuildRecord>(&value)
                    .map_err(|_| MessageCellError::InvalidRequest)?;
                if !self
                    .translation_cells
                    .contains_key(&imported.translation_cell)
                {
                    return Err(MessageCellError::InvalidRequest);
                }
                // Frozen until the Directory points here.
                imported.frozen = true;
                imported.moved = false;
                batch.put_value(GUILDS, key, &imported);
                record = Some(imported);
            } else {
                batch.put(table, key, value);
            }
        }
        self.store.write(batch).map_err(storage)?;
        if let Some(record) = record {
            self.signals.publish(&record.guild.id, record.last_seq);
        }
        Ok(())
    }

    pub(crate) async fn finish_move(&self, guild: &str, moved: bool) -> CellResult<()> {
        let last_seq = {
            let _guard = self.lock(guild).await;
            let mut record = self.raw_guild(guild)?;
            record.frozen = false;
            if moved {
                let prefix = guild_key(guild);
                let mut batch = Batch::new();
                for table in GUILD_TABLES.iter().filter(|table| **table != GUILDS) {
                    batch.delete_prefix(table, prefix.clone());
                }
                record.moved = true;
                batch.put_value(GUILDS, guild_key(guild), &record);
                self.store.write(batch).map_err(storage)?;
                self.marks.forget(&guild.to_string());
            } else {
                self.put_guild(&record)?;
            }
            record.last_seq
        };
        // Writers waiting for the move go on (or hear it moved); long polls
        // wake to find out.
        self.moves.send_modify(|moves| *moves += 1);
        self.signals.publish(&guild.to_string(), last_seq + 1);
        Ok(())
    }

    pub(crate) async fn discard_guild(&self, guild: &str) -> CellResult<()> {
        let _guard = self.lock(guild).await;
        // Never a guild that lives here.
        if let Ok(record) = self.raw_guild(guild)
            && !record.frozen
            && !record.moved
        {
            return Err(MessageCellError::InvalidRequest);
        }
        let prefix = guild_key(guild);
        let mut batch = Batch::new();
        for table in GUILD_TABLES {
            batch.delete_prefix(table, prefix.clone());
        }
        self.store.write(batch).map_err(storage)?;
        self.marks.forget(&guild.to_string());
        Ok(())
    }
}
