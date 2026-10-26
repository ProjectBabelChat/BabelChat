# Storage

Code: `server/store`

## One process, one data directory

Each service process owns one data directory, opened as one RocksDB
instance: program and state move together. The all-in-one server keeps one
directory per service:

```
data/account  data/directory  data/message-cell-0  data/message-cell-1
data/translation-cell-0  data/translation-cell-1
```

A guild or space is a key prefix inside its cell's database, not a database of
its own (thousands of RocksDB instances would waste memtables and file
handles).

Services use the `KvStore` trait: `RocksStore` in production, `MemStore` in
tests. Another embedded LSM (fjall, for instance) could replace RocksDB behind
it.

## Why an LSM store fits

Chat is append-mostly and read by range: history pages and event logs are
range scans over time-ordered keys. Discord stores messages in an LSM store
(ScyllaDB) for the same reason. Each guild has a single writer, so atomic
batches are enough; no transactions are needed.

## Keys and values

- `Key` builds keys whose byte order matches component order: big-endian
  integers, length-prefixed strings (the key of guild `ab` never prefixes the
  keys of guild `abc`).
- Values are a format byte followed by JSON (`encode` / `decode`). JSON keeps
  Phase 0 data inspectable; a binary format can replace it behind a new format
  byte. A newer binary must open an older data directory, so format changes
  come with a migration on open.

## The event log is the backbone

Each guild's log (Message Cell) and each space's log (Translation Cell) record
every change, written in the same batch as the change. One log serves several
purposes:

| Use | Status |
|---|---|
| Edge fan-out (`poll_events`) | Phase 0 |
| Outbox to other services | Phase 0 |
| Gateway resume from a sequence number | Phase 0 (per session) |
| Catch-up while moving a guild | planned |
| Replication to a standby cell | planned |

Logs are not trimmed yet.

## Moving a guild (planned)

```
1. snapshot; range-scan the guild's prefix in each table → SST files
2. ship them; IngestExternalFile on the target while the source keeps serving
3. stream log entries written since the snapshot
4. pause the guild's writes (Edges hold requests), ship the last entries,
   bump the placement epoch in the Directory, resume on the target
5. DeleteRange on the source after a grace period
```

PostgreSQL would need the same steps (export a guild's rows from every
table); a key prefix per guild makes it a range operation here.

## Durability (the cost of embedding)

RocksDB keeps one disk's worth of durability and has no replication; that is
the real trade-off against a database server.

| Stage | Plan | Lost on disk failure |
|---|---|---|
| Phase 0 | nothing beyond the disk | everything since the last manual backup |
| next | `BackupEngine` snapshots + log shipping to object storage | seconds |
| later | ship each guild's log to a standby cell | a few entries at failover |
| if needed | Raft per cell (e.g. openraft) | nothing, at 3× the cost |

The Account service is the exception: relational, transactional, small, and
closed — it uses PostgreSQL in production.

## Crash safety

Every state change is one atomic batch with its log entry and, when other
services mirror it, its outbox entry; queued translation jobs are stored with
their full request. A process killed at any point resumes where it stopped
(`data_survives_a_crash` in the end-to-end tests kills the server and starts
it again).
