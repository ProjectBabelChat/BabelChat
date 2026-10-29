//! Trims guild logs.
//!
//! A guild's log feeds Edges (who follow it closely) and the outbox. Once
//! both are past an entry, it only takes space; entries older than
//! [`LOG_RETENTION`] go, except those the outbox has not delivered yet. An
//! Edge that falls further behind learns so from `trimmed` in the next poll
//! and has its sessions reload.
//!
//! Uploads nobody sent go too, after [`UPLOADS_KEPT`]; the Media service
//! deletes their files on its own.

use std::{collections::HashMap, sync::Arc, time::Duration};

use babel_common::now_ms;
use babel_store::{Batch, Scan, StoreError};

use crate::{
    Inner,
    records::{GUILDS, GuildRecord, LOG, OUTBOX, OutboxEntry, UPLOADS, Upload, guild_key, log_key},
};

/// How long log entries are kept: well past any pause a follower recovers
/// from without reloading.
pub const LOG_RETENTION: Duration = Duration::from_secs(60 * 60);
/// How long an upload waits for the message it goes with.
pub const UPLOADS_KEPT: Duration = Duration::from_secs(24 * 60 * 60);
const TRIM_EVERY: Duration = Duration::from_secs(5 * 60);
const PAGE: usize = 500;

pub(crate) async fn run(inner: Arc<Inner>) {
    loop {
        tokio::time::sleep(TRIM_EVERY).await;
        match inner.trim_logs(LOG_RETENTION).await {
            Ok(0) => {}
            Ok(entries) => tracing::info!(entries, "trimmed guild logs"),
            Err(error) => tracing::error!(%error, "trimming guild logs failed"),
        }
        match inner.expire_uploads(UPLOADS_KEPT) {
            Ok(0) => {}
            Ok(uploads) => tracing::info!(uploads, "forgot uploads nobody sent"),
            Err(error) => tracing::error!(%error, "forgetting old uploads failed"),
        }
    }
}

impl Inner {
    /// Forgets uploads older than `kept`. Returns how many went.
    pub(crate) fn expire_uploads(&self, kept: Duration) -> Result<usize, StoreError> {
        let before = now_ms() - kept.as_millis() as i64;
        let mut expired = 0;
        let mut after = None::<Vec<u8>>;
        loop {
            let mut scan = Scan::prefix(UPLOADS, &[]).limit(PAGE);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan_values::<Upload>(scan)?;
            let Some((last, _)) = page.last() else {
                return Ok(expired);
            };
            after = Some(last.clone());
            let mut batch = Batch::new();
            for (key, upload) in page {
                if upload.uploaded_at < before {
                    batch.delete(UPLOADS, key);
                    expired += 1;
                }
            }
            self.store.write(batch)?;
        }
    }

    /// Trims every guild's log to `retention`. Returns how many entries
    /// went.
    pub(crate) async fn trim_logs(&self, retention: Duration) -> Result<usize, StoreError> {
        // The outbox still needs what it has not delivered.
        let mut pending = HashMap::<String, i64>::new();
        for (_, entry) in self
            .store
            .scan_values::<OutboxEntry>(Scan::prefix(OUTBOX, &[]))?
        {
            let oldest = pending.entry(entry.guild).or_insert(entry.seq);
            *oldest = (*oldest).min(entry.seq);
        }

        let mut trimmed = 0;
        let mut after = None::<Vec<u8>>;
        loop {
            let mut scan = Scan::prefix(GUILDS, &[]).limit(PAGE);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan_values::<GuildRecord>(scan)?;
            let Some((last, _)) = page.last() else {
                break;
            };
            after = Some(last.clone());

            for (_, record) in page {
                let guild = record.guild.id;
                let Some(mut until) = self.marks.mark(&guild, record.last_seq, retention) else {
                    continue;
                };
                if let Some(oldest) = pending.get(&guild) {
                    until = until.min(oldest - 1);
                }
                if until <= record.trimmed {
                    continue;
                }
                // Writers rewrite the record; trim under the same lock.
                let _guard = self.lock(&guild).await;
                let Some(mut record) = self
                    .store
                    .get_value::<GuildRecord>(GUILDS, &guild_key(&guild))?
                else {
                    continue;
                };
                let mut batch = Batch::new();
                batch.delete_range(
                    LOG,
                    log_key(&guild, record.trimmed + 1),
                    log_key(&guild, until + 1),
                );
                trimmed += (until - record.trimmed) as usize;
                record.trimmed = until;
                batch.put_value(GUILDS, guild_key(&guild), &record);
                self.store.write(batch)?;
            }
        }
        Ok(trimmed)
    }
}
