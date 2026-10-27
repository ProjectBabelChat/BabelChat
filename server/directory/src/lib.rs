//! The Directory: which cells hold each guild, and which guilds each user
//! belongs to.
//!
//! Placement is the one thing Edges must learn quickly when a guild is
//! created. Correctness never depends on how fast that news spreads:
//!
//! 1. the Edge that relayed `create_guild` gets the placement in the reply;
//! 2. other Edges receive it through `watch_placements` (a long poll);
//! 3. an Edge asked about a guild it has not seen calls `get_placement`.
//!
//! Guild IDs are random and placements never leave the internal network, so
//! clients cannot tell which cell holds a guild.
//!
//! Memberships are copied here from the Message Cells. Their changes form a
//! stream of their own (`watch_memberships`), which Edges follow to tell a
//! user's sessions about a guild joined through another Edge.
//!
//! ```text
//! table           key               value
//! placements      guild             Placement
//! versions        version           guild ID          (placement changes, in order)
//! memberships     user / guild      ()
//! membership_log  version           MembershipChange  (membership changes, in order)
//! ```
//!
//! See docs/architecture/directory.md.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use babel_api_directory::types::{
    DirectoryError, GuildRequest, MembershipBatch, MembershipChange, Placement, PlacementBatch,
};
use babel_api_message_cell::types::{GuildImport, MessageCell, NewGuild};
use babel_common::{LogMarks, LogSignals, poll_timeout, random_id};
use babel_store::{Batch, Key, KeyReader, KvStore, Scan, StoreError, Table};
use parking_lot::Mutex;

const PLACEMENTS: Table = "placements";
const VERSIONS: Table = "versions";
const MEMBERSHIPS: Table = "memberships";
const MEMBERSHIP_LOG: Table = "membership_log";

pub const TABLES: &[Table] = &[PLACEMENTS, VERSIONS, MEMBERSHIPS, MEMBERSHIP_LOG];

const WATCH_LIMIT: usize = 500;
/// How long membership changes are kept for Edges that fell behind.
pub const LOG_RETENTION: Duration = Duration::from_secs(60 * 60);
const TRIM_EVERY: Duration = Duration::from_secs(5 * 60);

fn storage(error: StoreError) -> DirectoryError {
    tracing::error!(%error, "directory storage failed");
    DirectoryError::Unavailable
}

/// The two streams watchers follow.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Stream {
    Placements,
    Memberships,
}

#[derive(Clone)]
pub struct Directory {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<dyn KvStore>,
    message_cells: Vec<(String, Arc<dyn MessageCell>)>,
    translation_cells: Vec<String>,
    /// Serializes guild creation, so versions are handed out in order.
    create: tokio::sync::Mutex<()>,
    state: Mutex<State>,
    signals: LogSignals<Stream>,
    /// How far the membership stream had grown at each trimming pass.
    marks: LogMarks<()>,
}

struct State {
    version: i64,
    /// Guilds per cell, to place new guilds on the least loaded one.
    load: HashMap<String, usize>,
    membership_version: i64,
    /// Membership changes up to this version were trimmed.
    membership_trimmed: i64,
}

/// The newest version in a versioned table.
fn last_version(store: &dyn KvStore, table: Table) -> Result<i64, StoreError> {
    Ok(store
        .scan(Scan::prefix(table, &[]).reverse().limit(1))?
        .first()
        .and_then(|(key, _)| KeyReader::new(key).u64())
        .map(|version| version as i64)
        .unwrap_or(0))
}

impl Directory {
    pub fn open(
        store: Arc<dyn KvStore>,
        message_cells: Vec<(String, Arc<dyn MessageCell>)>,
        translation_cells: Vec<String>,
    ) -> Result<Self, StoreError> {
        let mut load = HashMap::new();
        for (_, placement) in store.scan_values::<Placement>(Scan::prefix(PLACEMENTS, &[]))? {
            *load.entry(placement.message_cell).or_default() += 1;
            *load.entry(placement.translation_cell).or_default() += 1;
        }
        let version = last_version(&*store, VERSIONS)?;
        let membership_version = last_version(&*store, MEMBERSHIP_LOG)?;
        // Everything before the oldest change kept was trimmed.
        let membership_trimmed = store
            .scan(Scan::prefix(MEMBERSHIP_LOG, &[]).limit(1))?
            .first()
            .and_then(|(key, _)| KeyReader::new(key).u64())
            .map(|oldest| oldest as i64 - 1)
            .unwrap_or(membership_version);

        let signals = LogSignals::new();
        signals.publish(&Stream::Placements, version);
        signals.publish(&Stream::Memberships, membership_version);
        let inner = Arc::new(Inner {
            store,
            message_cells,
            translation_cells,
            create: tokio::sync::Mutex::new(()),
            state: Mutex::new(State {
                version,
                load,
                membership_version,
                membership_trimmed,
            }),
            signals,
            marks: LogMarks::default(),
        });
        tokio::spawn(trim(Arc::downgrade(&inner)));
        Ok(Self { inner })
    }

    /// Trims membership changes to `retention` now (it also happens every
    /// few minutes, to [`LOG_RETENTION`]). Returns how many went.
    pub fn trim_memberships(&self, retention: Duration) -> Result<usize, StoreError> {
        self.inner.trim_memberships(retention)
    }
}

impl Inner {
    fn least_loaded<'a>(&self, cells: impl Iterator<Item = &'a String>) -> Option<String> {
        let state = self.state.lock();
        cells
            .min_by_key(|cell| state.load.get(*cell).copied().unwrap_or(0))
            .cloned()
    }

    async fn create_guild(&self, request: GuildRequest) -> Result<Placement, DirectoryError> {
        let _guard = self.create.lock().await;

        let message_cell = self
            .least_loaded(self.message_cells.iter().map(|(name, _)| name))
            .ok_or(DirectoryError::NoCapacity)?;
        let translation_cell = self
            .least_loaded(self.translation_cells.iter())
            .ok_or(DirectoryError::NoCapacity)?;
        let cell = self
            .message_cells
            .iter()
            .find(|(name, _)| *name == message_cell)
            .map(|(_, cell)| cell.clone())
            .ok_or(DirectoryError::NoCapacity)?;

        let guild_id = random_id();
        // Create the guild first, then publish where it lives. A crash in
        // between leaves an unreachable guild on the cell, never a placement
        // pointing at nothing.
        cell.create_guild(NewGuild {
            guild_id: guild_id.clone(),
            translation_cell: translation_cell.clone(),
            name: request.name,
            description: request.description,
            owner_id: request.owner_id,
            owner_name: request.owner_name,
        })
        .await
        .map_err(|error| {
            tracing::warn!(?error, cell = %message_cell, "creating a guild failed");
            DirectoryError::Unavailable
        })?;

        let version = self.state.lock().version + 1;
        let placement = Placement {
            guild_id: guild_id.clone(),
            message_cell: message_cell.clone(),
            translation_cell: translation_cell.clone(),
            epoch: 1,
            version,
        };
        let mut batch = Batch::new();
        batch.put_value(PLACEMENTS, Key::new().str(&guild_id).build(), &placement);
        batch.put(
            VERSIONS,
            Key::new().u64(version as u64).build(),
            guild_id.into_bytes(),
        );
        self.store.write(batch).map_err(storage)?;

        {
            let mut state = self.state.lock();
            state.version = version;
            *state.load.entry(message_cell).or_default() += 1;
            *state.load.entry(translation_cell).or_default() += 1;
        }
        self.signals.publish(&Stream::Placements, version);
        Ok(placement)
    }

    fn message_cell(&self, name: &str) -> Result<Arc<dyn MessageCell>, DirectoryError> {
        self.message_cells
            .iter()
            .find(|(cell, _)| cell == name)
            .map(|(_, cell)| cell.clone())
            .ok_or(DirectoryError::NotFound)
    }

    /// Moves a guild to another Message Cell. See the Message Cell's
    /// `moving` module for the steps on the cells' side.
    async fn move_guild(&self, guild: &str, to: &str) -> Result<Placement, DirectoryError> {
        // One placement change at a time, so versions stay in order.
        let _guard = self.create.lock().await;
        let placement = self.placement(guild)?;
        if placement.message_cell == to {
            return Ok(placement);
        }
        let source = self.message_cell(&placement.message_cell)?;
        let target = self.message_cell(to)?;
        let failed = |step: &'static str| {
            move |error| {
                tracing::warn!(?error, %guild, step, "moving a guild failed");
                DirectoryError::Unavailable
            }
        };

        // Leftovers of an earlier attempt go first.
        target
            .discard_guild(guild.to_string())
            .await
            .map_err(failed("discard"))?;
        source
            .freeze_guild(guild.to_string())
            .await
            .map_err(failed("freeze"))?;
        let copied = async {
            let mut cursor = None;
            loop {
                let page = source.export_guild(guild.to_string(), cursor, 500).await?;
                target
                    .import_guild(GuildImport {
                        guild_id: guild.to_string(),
                        entries: page.entries,
                    })
                    .await?;
                match page.cursor {
                    Some(next) => cursor = Some(next),
                    None => return Ok(()),
                }
            }
        };
        if let Err(error) = copied.await {
            let _ = target.discard_guild(guild.to_string()).await;
            let _ = source.finish_move(guild.to_string(), false).await;
            return Err(failed("copy")(error));
        }

        // From here on the guild lives on `to`.
        let version = self.state.lock().version + 1;
        let moved = Placement {
            message_cell: to.to_string(),
            epoch: placement.epoch + 1,
            version,
            ..placement.clone()
        };
        let mut batch = Batch::new();
        batch.put_value(PLACEMENTS, Key::new().str(guild).build(), &moved);
        batch.delete(VERSIONS, Key::new().u64(placement.version as u64).build());
        batch.put(
            VERSIONS,
            Key::new().u64(version as u64).build(),
            guild.as_bytes().to_vec(),
        );
        if let Err(error) = self.store.write(batch) {
            let _ = target.discard_guild(guild.to_string()).await;
            let _ = source.finish_move(guild.to_string(), false).await;
            return Err(storage(error));
        }
        {
            let mut state = self.state.lock();
            state.version = version;
            if let Some(load) = state.load.get_mut(&placement.message_cell) {
                *load = load.saturating_sub(1);
            }
            *state.load.entry(to.to_string()).or_default() += 1;
        }
        self.signals.publish(&Stream::Placements, version);

        // Writes resume on the new cell; writers still waiting at the old
        // one hear that the guild moved and follow the new placement.
        for attempt in 1..=5 {
            let thawed = target.finish_move(guild.to_string(), false).await;
            let dropped = match thawed {
                Ok(()) => source.finish_move(guild.to_string(), true).await,
                Err(error) => Err(error),
            };
            match dropped {
                Ok(()) => break,
                Err(error) if attempt == 5 => {
                    tracing::error!(?error, %guild, "the guild moved, but finishing the move failed; run it again");
                    return Err(DirectoryError::Unavailable);
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            }
        }
        tracing::info!(%guild, from = %placement.message_cell, %to, "moved a guild");
        Ok(moved)
    }

    fn placement(&self, guild: &str) -> Result<Placement, DirectoryError> {
        self.store
            .get_value(PLACEMENTS, &Key::new().str(guild).build())
            .map_err(storage)?
            .ok_or(DirectoryError::NotFound)
    }

    fn placements_after(&self, after: i64) -> Result<Vec<Placement>, DirectoryError> {
        let after_key = Key::new().u64(after.max(0) as u64).build();
        let guilds = self
            .store
            .scan(
                Scan::prefix(VERSIONS, &[])
                    .after(&after_key)
                    .limit(WATCH_LIMIT),
            )
            .map_err(storage)?;
        guilds
            .into_iter()
            .map(|(_, guild)| self.placement(&String::from_utf8_lossy(&guild)))
            .collect()
    }

    async fn watch_placements(
        &self,
        after: i64,
        wait_ms: i64,
    ) -> Result<PlacementBatch, DirectoryError> {
        let mut placements = self.placements_after(after)?;
        if placements.is_empty()
            && self
                .signals
                .wait_past(&Stream::Placements, after, poll_timeout(wait_ms))
                .await
        {
            placements = self.placements_after(after)?;
        }
        Ok(PlacementBatch {
            version: placements
                .iter()
                .map(|placement| placement.version)
                .max()
                .unwrap_or(after),
            placements,
        })
    }

    /// Records that `user` joined or left `guild`, and wakes watchers.
    fn change_membership(
        &self,
        user: String,
        guild: String,
        joined: bool,
    ) -> Result<(), DirectoryError> {
        // One writer at a time, so versions are handed out in order.
        let mut state = self.state.lock();
        let change = MembershipChange {
            user_id: user,
            guild_id: guild,
            joined,
            version: state.membership_version + 1,
        };
        let key = Key::new()
            .str(&change.user_id)
            .str(&change.guild_id)
            .build();
        let mut batch = Batch::new();
        if joined {
            batch.put(MEMBERSHIPS, key, Vec::new());
        } else {
            batch.delete(MEMBERSHIPS, key);
        }
        batch.put_value(
            MEMBERSHIP_LOG,
            Key::new().u64(change.version as u64).build(),
            &change,
        );
        self.store.write(batch).map_err(storage)?;
        state.membership_version = change.version;
        drop(state);
        self.signals.publish(&Stream::Memberships, change.version);
        Ok(())
    }

    fn memberships_after(&self, after: i64) -> Result<Vec<MembershipChange>, DirectoryError> {
        let after_key = Key::new().u64(after.max(0) as u64).build();
        Ok(self
            .store
            .scan_values::<MembershipChange>(
                Scan::prefix(MEMBERSHIP_LOG, &[])
                    .after(&after_key)
                    .limit(WATCH_LIMIT),
            )
            .map_err(storage)?
            .into_iter()
            .map(|(_, change)| change)
            .collect())
    }

    async fn watch_memberships(
        &self,
        after: i64,
        wait_ms: i64,
    ) -> Result<MembershipBatch, DirectoryError> {
        let head = self.state.lock().membership_version;
        if after >= head {
            if after > head {
                // A follower starting at the end.
                return Ok(MembershipBatch {
                    changes: Vec::new(),
                    version: head,
                    trimmed: self.state.lock().membership_trimmed,
                });
            }
            self.signals
                .wait_past(&Stream::Memberships, after, poll_timeout(wait_ms))
                .await;
        }
        let changes = self.memberships_after(after)?;
        Ok(MembershipBatch {
            version: changes.last().map(|change| change.version).unwrap_or(after),
            changes,
            trimmed: self.state.lock().membership_trimmed,
        })
    }

    /// Trims membership changes older than `retention`. Returns how many
    /// went.
    fn trim_memberships(&self, retention: Duration) -> Result<usize, StoreError> {
        let mut state = self.state.lock();
        let Some(until) = self.marks.mark(&(), state.membership_version, retention) else {
            return Ok(0);
        };
        // The newest change stays: it is where versions carry on from
        // after a restart.
        let until = until.min(state.membership_version - 1);
        if until <= state.membership_trimmed {
            return Ok(0);
        }
        let mut batch = Batch::new();
        batch.delete_range(
            MEMBERSHIP_LOG,
            Key::new().u64(state.membership_trimmed as u64 + 1).build(),
            Key::new().u64(until as u64 + 1).build(),
        );
        self.store.write(batch)?;
        let trimmed = (until - state.membership_trimmed) as usize;
        state.membership_trimmed = until;
        Ok(trimmed)
    }
}

/// Trims the membership stream every few minutes.
async fn trim(inner: std::sync::Weak<Inner>) {
    loop {
        tokio::time::sleep(TRIM_EVERY).await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        match inner.trim_memberships(LOG_RETENTION) {
            Ok(0) => {}
            Ok(changes) => tracing::info!(changes, "trimmed membership changes"),
            Err(error) => tracing::error!(%error, "trimming membership changes failed"),
        }
    }
}

#[async_trait]
impl babel_api_directory::types::Directory for Directory {
    async fn create_guild(&self, body: GuildRequest) -> Result<Placement, DirectoryError> {
        self.inner.create_guild(body).await
    }

    async fn get_placement(&self, guild_id: String) -> Result<Placement, DirectoryError> {
        self.inner.placement(&guild_id)
    }

    async fn watch_placements(
        &self,
        after: i64,
        wait_ms: i64,
    ) -> Result<PlacementBatch, DirectoryError> {
        self.inner.watch_placements(after, wait_ms).await
    }

    async fn add_membership(
        &self,
        user_id: String,
        guild_id: String,
    ) -> Result<(), DirectoryError> {
        self.inner.change_membership(user_id, guild_id, true)
    }

    async fn remove_membership(
        &self,
        user_id: String,
        guild_id: String,
    ) -> Result<(), DirectoryError> {
        self.inner.change_membership(user_id, guild_id, false)
    }

    async fn list_memberships(&self, user_id: String) -> Result<Vec<String>, DirectoryError> {
        let prefix = Key::new().str(&user_id).build();
        Ok(self
            .inner
            .store
            .scan(Scan::prefix(MEMBERSHIPS, &prefix))
            .map_err(storage)?
            .into_iter()
            .filter_map(|(key, _)| {
                let mut reader = KeyReader::new(&key);
                reader.str()?;
                reader.str().map(str::to_string)
            })
            .collect())
    }

    async fn watch_memberships(
        &self,
        after: i64,
        wait_ms: i64,
    ) -> Result<MembershipBatch, DirectoryError> {
        self.inner.watch_memberships(after, wait_ms).await
    }

    async fn move_guild(
        &self,
        guild_id: String,
        message_cell: String,
    ) -> Result<Placement, DirectoryError> {
        self.inner.move_guild(&guild_id, &message_cell).await
    }
}

#[cfg(test)]
mod tests {
    use babel_api_directory::types::Directory as _;
    use babel_store::MemStore;

    use super::*;

    #[tokio::test]
    async fn membership_changes_stream_and_trim() {
        let directory = Directory::open(Arc::new(MemStore::new()), Vec::new(), Vec::new()).unwrap();
        // A follower starting at the end.
        let start = directory.watch_memberships(i64::MAX, 0).await.unwrap();
        assert_eq!(start.version, 0);

        directory
            .add_membership("alice".into(), "g".into())
            .await
            .unwrap();
        directory
            .add_membership("bob".into(), "g".into())
            .await
            .unwrap();
        directory
            .remove_membership("alice".into(), "g".into())
            .await
            .unwrap();
        let batch = directory.watch_memberships(0, 0).await.unwrap();
        assert_eq!(
            batch
                .changes
                .iter()
                .map(|change| (change.user_id.as_str(), change.joined))
                .collect::<Vec<_>>(),
            [("alice", true), ("bob", true), ("alice", false)]
        );
        assert_eq!(batch.version, 3);
        assert_eq!(
            directory.list_memberships("bob".into()).await.unwrap(),
            ["g"]
        );
        assert!(
            directory
                .list_memberships("alice".into())
                .await
                .unwrap()
                .is_empty()
        );

        // Trimming keeps the newest change, and says what went.
        assert_eq!(directory.trim_memberships(Duration::ZERO).unwrap(), 2);
        let behind = directory.watch_memberships(0, 0).await.unwrap();
        assert_eq!(behind.trimmed, 2);
        assert_eq!(behind.changes.len(), 1);
    }
}
