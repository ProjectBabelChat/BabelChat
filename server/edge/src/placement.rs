//! The Edge's copy of the placement table.
//!
//! Three ways a placement reaches an Edge, fastest first:
//!
//! 1. `insert`: this Edge relayed the guild's creation (read-your-writes);
//! 2. the watch loop: the Directory streams every committed placement;
//! 3. `resolve` on a miss: ask the Directory directly.
//!
//! Misses are remembered briefly, and concurrent misses for one guild share
//! a single lookup, so random guild IDs cannot flood the Directory.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use babel_api_directory::types::{Directory, DirectoryError, Placement};
use parking_lot::{Mutex, RwLock};

const NEGATIVE_TTL: Duration = Duration::from_secs(5);
const WATCH_WAIT_MS: i64 = 25_000;

pub struct Placements {
    directory: Arc<dyn Directory>,
    table: RwLock<HashMap<String, Placement>>,
    version: Mutex<i64>,
    misses: Mutex<HashMap<String, Instant>>,
    lookups: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Placements {
    /// Creates the cache and starts following the Directory.
    pub fn start(directory: Arc<dyn Directory>) -> Arc<Self> {
        let placements = Arc::new(Self {
            directory,
            table: RwLock::default(),
            version: Mutex::new(0),
            misses: Mutex::default(),
            lookups: Mutex::default(),
        });
        tokio::spawn(placements.clone().watch());
        placements
    }

    async fn watch(self: Arc<Self>) {
        loop {
            let after = *self.version.lock();
            let batch = self.directory.watch_placements(after, WATCH_WAIT_MS).await;
            for placement in batch.placements {
                self.insert(placement);
            }
            let mut version = self.version.lock();
            *version = (*version).max(batch.version);
        }
    }

    pub fn insert(&self, placement: Placement) {
        self.misses.lock().remove(&placement.guild_id);
        let mut table = self.table.write();
        let newer = table
            .get(&placement.guild_id)
            .is_none_or(|known| known.version <= placement.version);
        if newer {
            table.insert(placement.guild_id.clone(), placement);
        }
    }

    pub async fn resolve(&self, guild: &str) -> Result<Placement, DirectoryError> {
        if let Some(placement) = self.table.read().get(guild) {
            return Ok(placement.clone());
        }
        if self
            .misses
            .lock()
            .get(guild)
            .is_some_and(|since| since.elapsed() < NEGATIVE_TTL)
        {
            return Err(DirectoryError::NotFound);
        }

        // One lookup per guild at a time; later callers reuse its result.
        let lookup = self
            .lookups
            .lock()
            .entry(guild.to_string())
            .or_default()
            .clone();
        let _guard = lookup.lock().await;
        if let Some(placement) = self.table.read().get(guild) {
            return Ok(placement.clone());
        }

        let result = self.directory.get_placement(guild.to_string()).await;
        self.lookups.lock().remove(guild);
        match result {
            Ok(placement) => {
                self.insert(placement.clone());
                Ok(placement)
            }
            Err(DirectoryError::NotFound) => {
                self.misses.lock().insert(guild.to_string(), Instant::now());
                Err(DirectoryError::NotFound)
            }
            Err(error) => Err(error),
        }
    }
}
