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
//! ```text
//! table        key               value
//! placements   guild             Placement
//! versions     version           guild ID   (placement changes, in order)
//! memberships  user / guild      ()
//! ```
//!
//! See docs/architecture/directory.md.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use babel_api_directory::types::{DirectoryError, GuildRequest, Placement, PlacementBatch};
use babel_api_message_cell::types::{MessageCell, NewGuild};
use babel_common::{LogSignals, poll_timeout, random_id};
use babel_store::{Batch, Key, KeyReader, KvStore, Scan, StoreError, Table};
use parking_lot::Mutex;

const PLACEMENTS: Table = "placements";
const VERSIONS: Table = "versions";
const MEMBERSHIPS: Table = "memberships";

pub const TABLES: &[Table] = &[PLACEMENTS, VERSIONS, MEMBERSHIPS];

const WATCH_LIMIT: usize = 500;

fn storage(error: StoreError) -> DirectoryError {
    tracing::error!(%error, "directory storage failed");
    DirectoryError::Unavailable
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
    signals: LogSignals<()>,
}

struct State {
    version: i64,
    /// Guilds per cell, to place new guilds on the least loaded one.
    load: HashMap<String, usize>,
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
        let version = store
            .scan(Scan::prefix(VERSIONS, &[]).reverse().limit(1))?
            .first()
            .and_then(|(key, _)| KeyReader::new(key).u64())
            .map(|version| version as i64)
            .unwrap_or(0);

        let signals = LogSignals::new();
        signals.publish(&(), version);
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                message_cells,
                translation_cells,
                create: tokio::sync::Mutex::new(()),
                state: Mutex::new(State { version, load }),
                signals,
            }),
        })
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
        self.signals.publish(&(), version);
        Ok(placement)
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

    async fn watch_placements(&self, after: i64, wait_ms: i64) -> PlacementBatch {
        let mut placements = self.placements_after(after).unwrap_or_default();
        if placements.is_empty()
            && self
                .signals
                .wait_past(&(), after, poll_timeout(wait_ms))
                .await
        {
            placements = self.placements_after(after).unwrap_or_default();
        }
        PlacementBatch {
            version: placements
                .iter()
                .map(|placement| placement.version)
                .max()
                .unwrap_or(after),
            placements,
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

    async fn watch_placements(&self, after: i64, wait_ms: i64) -> PlacementBatch {
        self.inner.watch_placements(after, wait_ms).await
    }

    async fn add_membership(&self, user_id: String, guild_id: String) {
        let mut batch = Batch::new();
        batch.put(
            MEMBERSHIPS,
            Key::new().str(&user_id).str(&guild_id).build(),
            Vec::new(),
        );
        if let Err(error) = self.inner.store.write(batch) {
            tracing::error!(%error, "recording a membership failed");
        }
    }

    async fn remove_membership(&self, user_id: String, guild_id: String) {
        let mut batch = Batch::new();
        batch.delete(MEMBERSHIPS, Key::new().str(&user_id).str(&guild_id).build());
        if let Err(error) = self.inner.store.write(batch) {
            tracing::error!(%error, "removing a membership failed");
        }
    }

    async fn list_memberships(&self, user_id: String) -> Vec<String> {
        let prefix = Key::new().str(&user_id).build();
        self.inner
            .store
            .scan(Scan::prefix(MEMBERSHIPS, &prefix))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(key, _)| {
                let mut reader = KeyReader::new(&key);
                reader.str()?;
                reader.str().map(str::to_string)
            })
            .collect()
    }
}
