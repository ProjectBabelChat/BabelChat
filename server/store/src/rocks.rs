use std::path::Path;

use rocksdb::{ColumnFamilyDescriptor, DB, Direction, IteratorMode, Options, WriteBatch};

use crate::{Batch, KvStore, Op, Result, Scan, StoreError, Table, prefix_end};

/// RocksDB with one column family per table.
pub struct RocksStore {
    db: DB,
}

impl RocksStore {
    /// Opens (or creates) the store in `path` with the given tables.
    pub fn open(path: &Path, tables: &[Table]) -> Result<Self> {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        let families = tables
            .iter()
            .map(|table| ColumnFamilyDescriptor::new(*table, Options::default()));
        let db = DB::open_cf_descriptors(&options, path, families)?;
        Ok(Self { db })
    }

    fn family(&self, table: Table) -> Result<&rocksdb::ColumnFamily> {
        self.db
            .cf_handle(table)
            .ok_or(StoreError::UnknownTable(table))
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
