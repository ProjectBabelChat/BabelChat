use std::path::Path;

use rocksdb::{
    Cache, ColumnFamilyDescriptor, DB, DBCompressionType, Direction, Env, IteratorMode, Options,
    WriteBatch,
    backup::{BackupEngine, BackupEngineOptions, RestoreOptions},
    checkpoint::Checkpoint,
};

use crate::{Batch, KvStore, Op, Result, Scan, StoreError, Table, prefix_end};

/// How long write-ahead log files are kept after they stop being needed
/// locally, so a standby that fell behind can still catch up from them.
const WAL_TTL_SECS: u64 = 6 * 60 * 60;
const WAL_LIMIT_MB: u64 = 4 * 1024;

/// RocksDB with one column family per table.
pub struct RocksStore {
    pub(crate) db: DB,
}

/// One write of the primary, as a standby replays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// Sequence number of the write's first operation.
    pub seq: u64,
    /// The write batch, in RocksDB's own encoding.
    pub data: Vec<u8>,
}

fn options() -> Options {
    let mut options = Options::default();
    options.create_if_missing(true);
    options.create_missing_column_families(true);
    options.set_wal_ttl_seconds(WAL_TTL_SECS);
    options.set_wal_size_limit_mb(WAL_LIMIT_MB);
    options
}

/// A table whose values are files (attachments): values from 64 KB up go to
/// blob files beside the LSM tree, which compactions do not rewrite, and
/// are reclaimed as their keys are deleted. They are stored uncompressed:
/// images and archives already are.
fn blob_table_options() -> Options {
    let mut options = Options::default();
    options.set_enable_blob_files(true);
    options.set_min_blob_size(64 * 1024);
    options.set_blob_file_size(256 * 1024 * 1024);
    options.set_blob_compression_type(DBCompressionType::None);
    options.set_enable_blob_gc(true);
    options.set_blob_gc_age_cutoff(0.25);
    options
}

impl RocksStore {
    /// Opens (or creates) the store in `path` with the given tables.
    pub fn open(path: &Path, tables: &[Table]) -> Result<Self> {
        Self::open_with_blobs(path, tables, &[])
    }

    /// Like [`Self::open`]; the tables in `blob_tables` hold files, kept in
    /// blob files (see `blob_table_options`).
    pub fn open_with_blobs(path: &Path, tables: &[Table], blob_tables: &[Table]) -> Result<Self> {
        let families = tables.iter().map(|table| {
            let options = if blob_tables.contains(table) {
                blob_table_options()
            } else {
                Options::default()
            };
            ColumnFamilyDescriptor::new(*table, options)
        });
        let db = DB::open_cf_descriptors(&options(), path, families)?;
        Ok(Self { db })
    }

    /// Opens a store with whatever tables it has, for replaying a primary's
    /// writes into it (a standby does not know which service it copies).
    /// Each table keeps the options the primary gave it (its checkpoint
    /// carries them), blob files included.
    pub fn open_existing(path: &Path) -> Result<Self> {
        let (_, families) =
            Options::load_latest(path, Env::new()?, true, Cache::new_lru_cache(8 << 20))?;
        let db = DB::open_cf_descriptors(&options(), path, families)?;
        Ok(Self { db })
    }

    /// Where the store lives.
    pub fn path(&self) -> &Path {
        self.db.path()
    }

    fn family(&self, table: Table) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(table)
            .ok_or(StoreError::UnknownTable(table))
    }

    // ---- Replication ------------------------------------------------------

    /// Sequence number of the last write.
    pub fn latest_sequence(&self) -> u64 {
        self.db.latest_sequence_number()
    }

    /// Writes after `seq`, oldest first, up to about `max_bytes`. Fails if
    /// the write-ahead log no longer goes back that far.
    pub fn updates_since(&self, seq: u64, max_bytes: usize) -> Result<Vec<Update>> {
        let mut updates = Vec::new();
        let mut bytes = 0;
        if seq >= self.latest_sequence() {
            return Ok(updates);
        }
        for update in self.db.get_updates_since(seq)? {
            let (seq, batch) = update?;
            bytes += batch.data().len();
            updates.push(Update {
                seq,
                data: batch.data().to_vec(),
            });
            if bytes >= max_bytes {
                break;
            }
        }
        Ok(updates)
    }

    /// Replays a primary's write. A standby applies them in order, so its
    /// sequence numbers keep matching the primary's.
    pub fn apply(&self, update: &Update) -> Result<()> {
        Ok(self.db.write(WriteBatch::from_data(&update.data))?)
    }

    /// A consistent copy of the store in `path` (which must not exist):
    /// what a new standby starts from.
    pub fn checkpoint(&self, path: &Path) -> Result<()> {
        Ok(Checkpoint::new(&self.db)?.create_checkpoint(path)?)
    }

    // ---- Backups ----------------------------------------------------------

    /// Adds a backup to `dir` and keeps the newest `keep` of them. Backups
    /// share unchanged files, so frequent ones are cheap.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<()> {
        let mut engine = BackupEngine::open(&BackupEngineOptions::new(dir)?, &Env::new()?)?;
        engine.create_new_backup_flush(&self.db, true)?;
        engine.purge_old_backups(keep)?;
        Ok(())
    }

    /// Restores the newest backup in `backups` into `path`, which must not
    /// hold a store that is open.
    pub fn restore(backups: &Path, path: &Path) -> Result<()> {
        let mut engine = BackupEngine::open(&BackupEngineOptions::new(backups)?, &Env::new()?)?;
        engine.restore_from_latest_backup(path, path, &RestoreOptions::default())?;
        Ok(())
    }
}

impl KvStore for RocksStore {
    fn get(&self, table: Table, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.db.get_cf(self.family(table)?, key)?)
    }

    fn write(&self, batch: Batch) -> Result<()> {
        let mut write = WriteBatch::default();
        for op in batch.ops {
            match op {
                Op::Put(table, key, value) => write.put_cf(self.family(table)?, key, value),
                Op::Delete(table, key) => write.delete_cf(self.family(table)?, key),
                Op::DeleteRange(table, start, end) => {
                    write.delete_range_cf(self.family(table)?, start, end)
                }
            }
        }
        Ok(self.db.write(write)?)
    }

    fn scan(&self, scan: Scan<'_>) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let family = self.family(scan.table)?;
        let upper = prefix_end(scan.prefix);

        let mode = match (scan.reverse, scan.after) {
            (false, Some(after)) => IteratorMode::From(after, Direction::Forward),
            (false, None) => IteratorMode::From(scan.prefix, Direction::Forward),
            (true, Some(after)) => IteratorMode::From(after, Direction::Reverse),
            (true, None) => match &upper {
                Some(upper) => IteratorMode::From(upper, Direction::Reverse),
                None => IteratorMode::End,
            },
        };

        let mut entries = Vec::new();
        for item in self.db.iterator_cf(family, mode) {
            let (key, value) = item?;
            if Some(key.as_ref()) == scan.after || Some(key.as_ref()) == upper.as_deref() {
                continue;
            }
            if !key.starts_with(scan.prefix) {
                // Reverse seeks may land just past the prefix; forward scans
                // are done once they leave it.
                if scan.reverse && key.as_ref() > scan.prefix {
                    continue;
                }
                break;
            }
            entries.push((key.into_vec(), value.into_vec()));
            if entries.len() >= scan.limit {
                break;
            }
        }
        Ok(entries)
    }
}
