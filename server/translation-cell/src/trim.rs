//! Trims space logs.
//!
//! Only Edges read a space's log, and they follow it closely. Entries older
//! than [`LOG_RETENTION`] go; an Edge that falls further behind learns so
//! from `trimmed` in the next poll and has its sessions reload.

use std::{sync::Arc, time::Duration};

use babel_store::{Batch, KeyReader, Scan, StoreError};

use crate::{
    Inner,
    records::{EVENTS, SPACES, SpaceRecord, event_key, space_key},
};

/// How long log entries are kept: well past any pause a follower recovers
/// from without reloading.
pub const LOG_RETENTION: Duration = Duration::from_secs(60 * 60);
const TRIM_EVERY: Duration = Duration::from_secs(5 * 60);
const PAGE: usize = 500;

pub(crate) async fn run(inner: Arc<Inner>) {
    loop {
        tokio::time::sleep(TRIM_EVERY).await;
        match inner.trim_logs(LOG_RETENTION).await {
            Ok(0) => {}
            Ok(entries) => tracing::info!(entries, "trimmed space logs"),
            Err(error) => tracing::error!(%error, "trimming space logs failed"),
        }
    }
}

impl Inner {
    /// Trims every space's log to `retention`. Returns how many entries
    /// went.
    pub(crate) async fn trim_logs(&self, retention: Duration) -> Result<usize, StoreError> {
        let mut trimmed = 0;
        let mut after = None::<Vec<u8>>;
        loop {
            let mut scan = Scan::prefix(SPACES, &[]).limit(PAGE);
            if let Some(after) = &after {
                scan = scan.after(after);
            }
            let page = self.store.scan_values::<SpaceRecord>(scan)?;
            let Some((last, _)) = page.last() else {
                break;
            };
            after = Some(last.clone());

            for (key, record) in page {
                let mut reader = KeyReader::new(&key);
                let (Some(tenant), Some(space)) = (reader.str(), reader.str()) else {
                    continue;
                };
                let (tenant, space) = (tenant.to_string(), space.to_string());
                let log = (tenant.clone(), space.clone());
                let Some(until) = self.marks.mark(&log, record.last_event, retention) else {
                    continue;
                };
                if until <= record.trimmed {
                    continue;
                }
                // Writers rewrite the record; trim under the same lock.
                let _guard = self.lock(&tenant, &space).await;
                let Some(mut record) = self
                    .store
                    .get_value::<SpaceRecord>(SPACES, &space_key(&tenant, &space))?
                else {
                    continue;
                };
                let mut batch = Batch::new();
                batch.delete_range(
                    EVENTS,
                    event_key(&tenant, &space, record.trimmed + 1),
                    event_key(&tenant, &space, until + 1),
                );
                trimmed += (until - record.trimmed) as usize;
                record.trimmed = until;
                batch.put_value(SPACES, space_key(&tenant, &space), &record);
                self.store.write(batch)?;
            }
        }
        Ok(trimmed)
    }
}
