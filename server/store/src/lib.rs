//! Embedded key-value storage for cells.
//!
//! Each server process owns exactly one data directory, opened here as one
//! RocksDB instance: program and state move together. Guilds and spaces are
//! key prefixes inside it, so a single guild can still be exported or deleted
//! as a range.
//!
//! Services talk to [`KvStore`], not to RocksDB, so tests run on
//! [`MemStore`] and another LSM engine could replace RocksDB later.
//!
//! See docs/architecture/storage.md.

mod key;
mod mem;
mod rocks;

pub use key::{Key, KeyReader};
pub use mem::MemStore;
pub use rocks::RocksStore;
use serde::{Serialize, de::DeserializeOwned};

/// A column family. Each service declares the tables it uses.
pub type Table = &'static str;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("rocksdb: {0}")]
    Rocks(#[from] rocksdb::Error),
    #[error("unknown table {0:?}")]
    UnknownTable(Table),
    #[error("corrupt value: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Writes applied atomically.
#[derive(Debug, Default)]
pub struct Batch {
    ops: Vec<Op>,
}

#[derive(Debug)]
enum Op {
    Put(Table, Vec<u8>, Vec<u8>),
    Delete(Table, Vec<u8>),
    /// Deletes keys in `[start, end)`.
    DeleteRange(Table, Vec<u8>, Vec<u8>),
}

impl Batch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, table: Table, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.ops.push(Op::Put(table, key.into(), value.into()));
    }

    /// Puts a value encoded with [`encode`].
    pub fn put_value<T: Serialize>(&mut self, table: Table, key: impl Into<Vec<u8>>, value: &T) {
        self.put(table, key, encode(value));
    }

    pub fn delete(&mut self, table: Table, key: impl Into<Vec<u8>>) {
        self.ops.push(Op::Delete(table, key.into()));
    }

    /// Deletes every key starting with `prefix`.
    pub fn delete_prefix(&mut self, table: Table, prefix: impl Into<Vec<u8>>) {
        let prefix = prefix.into();
        match prefix_end(&prefix) {
            Some(end) => self.ops.push(Op::DeleteRange(table, prefix, end)),
            None => self
                .ops
                .push(Op::DeleteRange(table, prefix, vec![0xff; 64])),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// A range read within one key prefix.
#[derive(Debug, Clone, Copy)]
pub struct Scan<'a> {
    pub table: Table,
    pub prefix: &'a [u8],
    /// Start strictly after this key (in scan direction), if given.
    pub after: Option<&'a [u8]>,
    /// Newest (largest key) first.
    pub reverse: bool,
    pub limit: usize,
}

impl<'a> Scan<'a> {
    pub fn prefix(table: Table, prefix: &'a [u8]) -> Self {
        Self {
            table,
            prefix,
            after: None,
            reverse: false,
            limit: usize::MAX,
        }
    }

    pub fn after(mut self, key: &'a [u8]) -> Self {
        self.after = Some(key);
        self
    }

    pub fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

pub trait KvStore: Send + Sync + 'static {
    fn get(&self, table: Table, key: &[u8]) -> Result<Option<Vec<u8>>>;

    fn write(&self, batch: Batch) -> Result<()>;

    fn scan(&self, scan: Scan<'_>) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
}

impl dyn KvStore {
    /// Reads a value encoded with [`encode`].
    pub fn get_value<T: DeserializeOwned>(&self, table: Table, key: &[u8]) -> Result<Option<T>> {
        self.get(table, key)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    /// Scans and decodes values encoded with [`encode`].
    pub fn scan_values<T: DeserializeOwned>(&self, scan: Scan<'_>) -> Result<Vec<(Vec<u8>, T)>> {
        self.scan(scan)?
            .into_iter()
            .map(|(key, value)| Ok((key, decode(&value)?)))
            .collect()
    }
}

/// Format of stored values. Bump it when an encoding changes and migrate on
/// open: a newer binary must be able to open an older data directory.
pub const VALUE_FORMAT: u8 = 1;

/// Encodes a value as the format byte followed by JSON.
///
/// JSON keeps Phase 0 data inspectable; a compact binary format can replace
/// it behind a new [`VALUE_FORMAT`].
pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = vec![VALUE_FORMAT];
    serde_json::to_writer(&mut bytes, value).expect("values serialize to JSON");
    bytes
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    match bytes.split_first() {
        Some((&VALUE_FORMAT, json)) => {
            serde_json::from_slice(json).map_err(|error| StoreError::Corrupt(error.to_string()))
        }
        Some((format, _)) => Err(StoreError::Corrupt(format!(
            "unknown value format {format}"
        ))),
        None => Err(StoreError::Corrupt("empty value".into())),
    }
}

/// The smallest key greater than every key starting with `prefix`, or `None`
/// when no such key exists (the prefix is all `0xff`).
pub(crate) fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last < 0xff {
            end.push(last + 1);
            return Some(end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(store: &dyn KvStore) {
        let table = "t";
        let mut batch = Batch::new();
        for i in 0..5u64 {
            batch.put(table, Key::new().str("a").u64(i).build(), vec![i as u8]);
        }
        batch.put(table, Key::new().str("b").u64(0).build(), vec![9]);
        store.write(batch).unwrap();

        let prefix = Key::new().str("a").build();
        let all = store.scan(Scan::prefix(table, &prefix)).unwrap();
        assert_eq!(
            all.iter().map(|(_, v)| v[0]).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );

        let after = Key::new().str("a").u64(1).build();
        let forward = store
            .scan(Scan::prefix(table, &prefix).after(&after).limit(2))
            .unwrap();
        assert_eq!(
            forward.iter().map(|(_, v)| v[0]).collect::<Vec<_>>(),
            [2, 3]
        );

        let newest = store
            .scan(Scan::prefix(table, &prefix).reverse().limit(2))
            .unwrap();
        assert_eq!(newest.iter().map(|(_, v)| v[0]).collect::<Vec<_>>(), [4, 3]);

        let before = Key::new().str("a").u64(3).build();
        let older = store
            .scan(Scan::prefix(table, &prefix).after(&before).reverse())
            .unwrap();
        assert_eq!(
            older.iter().map(|(_, v)| v[0]).collect::<Vec<_>>(),
            [2, 1, 0]
        );

        let mut batch = Batch::new();
        batch.delete_prefix(table, prefix.clone());
        store.write(batch).unwrap();
        assert!(store.scan(Scan::prefix(table, &prefix)).unwrap().is_empty());
        assert_eq!(
            store
                .get(table, &Key::new().str("b").u64(0).build())
                .unwrap(),
            Some(vec![9])
        );
    }

    #[test]
    fn mem_store() {
        exercise(&MemStore::new());
    }

    #[test]
    fn rocks_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = RocksStore::open(dir.path(), &["t"]).unwrap();
        exercise(&store);
    }

    #[test]
    fn values_round_trip() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct V {
            a: u32,
        }
        let store: &dyn KvStore = &MemStore::new();
        let mut batch = Batch::new();
        batch.put_value("t", b"k".to_vec(), &V { a: 7 });
        store.write(batch).unwrap();
        assert_eq!(store.get_value::<V>("t", b"k").unwrap(), Some(V { a: 7 }));
    }

    #[test]
    fn prefix_end_increments() {
        assert_eq!(prefix_end(&[1, 2]), Some(vec![1, 3]));
        assert_eq!(prefix_end(&[1, 0xff]), Some(vec![2]));
        assert_eq!(prefix_end(&[0xff]), None);
    }
}
