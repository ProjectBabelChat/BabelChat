use std::collections::{BTreeMap, HashMap};

use parking_lot::Mutex;

use crate::{Batch, KvStore, Op, Result, Scan, Table, prefix_end};

/// In-memory store for tests.
#[derive(Default)]
pub struct MemStore {
    tables: Mutex<HashMap<Table, BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KvStore for MemStore {
    fn get(&self, table: Table, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self
            .tables
            .lock()
            .get(table)
            .and_then(|entries| entries.get(key).cloned()))
    }

    fn write(&self, batch: Batch) -> Result<()> {
        let mut tables = self.tables.lock();
        for op in batch.ops {
            match op {
                Op::Put(table, key, value) => {
                    tables.entry(table).or_default().insert(key, value);
                }
                Op::Delete(table, key) => {
                    tables.entry(table).or_default().remove(&key);
                }
                Op::DeleteRange(table, start, end) => {
                    let entries = tables.entry(table).or_default();
                    let doomed = entries
                        .range(start..end)
                        .map(|(key, _)| key.clone())
                        .collect::<Vec<_>>();
                    for key in doomed {
                        entries.remove(&key);
                    }
                }
            }
        }
        Ok(())
    }

    fn scan(&self, scan: Scan<'_>) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let tables = self.tables.lock();
        let Some(entries) = tables.get(scan.table) else {
            return Ok(Vec::new());
        };

        let in_range = |key: &Vec<u8>| {
            key.starts_with(scan.prefix)
                && match scan.after {
                    Some(after) if scan.reverse => key.as_slice() < after,
                    Some(after) => key.as_slice() > after,
                    None => true,
                }
        };

        let start = scan.prefix.to_vec();
        let range = match prefix_end(scan.prefix) {
            Some(end) => entries.range(start..end),
            None => entries.range(start..),
        };

        let collect = |iter: &mut dyn Iterator<Item = (&Vec<u8>, &Vec<u8>)>| {
            iter.filter(|(key, _)| in_range(key))
                .take(scan.limit)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        };

        Ok(if scan.reverse {
            collect(&mut range.rev())
        } else {
            collect(&mut { range })
        })
    }
}
