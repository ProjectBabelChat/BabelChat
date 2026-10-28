//! Periodic backups of a service's store.
//!
//! RocksDB's backup engine adds a backup to a directory (ideally one that
//! is not on the same disk: a mounted volume or object storage gateway)
//! and shares unchanged files between backups, so taking one every few
//! minutes is cheap. Together with a standby ([`crate::replication`]),
//! a lost disk costs seconds of writes at most.
//!
//! `babel-admin restore` puts the newest backup back into a data directory.

use std::{path::PathBuf, sync::Arc, time::Duration};

use babel_store::RocksStore;

/// Backs `store` up into `dir` every `every`, keeping the newest `keep`.
pub fn schedule(store: Arc<RocksStore>, dir: PathBuf, every: Duration, keep: usize) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(every).await;
            let (store, dir_) = (store.clone(), dir.clone());
            let started = std::time::Instant::now();
            match tokio::task::spawn_blocking(move || store.backup(&dir_, keep)).await {
                Ok(Ok(())) => {
                    tracing::info!(
                        dir = %dir.display(),
                        took_ms = started.elapsed().as_millis() as u64,
                        "backed up"
                    );
                    metrics::gauge!("babel_backup_last_success_timestamp_seconds")
                        .set(babel_common::now_ms() as f64 / 1000.0);
                    metrics::gauge!("babel_backup_last_duration_seconds")
                        .set(started.elapsed().as_secs_f64());
                }
                Ok(Err(error)) => {
                    tracing::error!(%error, "backing up failed");
                    metrics::counter!("babel_backup_failures_total").increment(1);
                }
                Err(error) => {
                    tracing::error!(%error, "backing up failed");
                    metrics::counter!("babel_backup_failures_total").increment(1);
                }
            }
        }
    });
}
