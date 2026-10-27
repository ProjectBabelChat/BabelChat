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
  come with a migration on open. Fields added to stored types are optional
  (`?` in TYML), so records written before they existed still decode
  (`Message.mentions`, `Channel.last_message_id`); new tables are created on
  open.

## The event log is the backbone

Each guild's log (Message Cell) and each space's log (Translation Cell) record
every change, written in the same batch as the change. One log serves several
purposes:

| Use | Which logs |
|---|---|
| Edge fan-out (`poll_events`) | every event |
| Per-member events across Edges (read markers, notification settings) | Message Cell logs |
| Outbox to other services | Message Cell logs |
| Following a guild to its new cell | sequence numbers move with it |

Logs keep an hour (`trim.rs` in each cell): long enough for any follower that
is only briefly away, except entries the outbox has not delivered yet. Each
poll says how far its log was trimmed, so a follower knows when it missed
something. Log entries carry no time; each trimming pass remembers how far
every log had grown (`LogMarks`), and trims to where it was an hour ago.

## Moving a guild

```
1. freeze the guild on its cell: writes wait, the outbox drains
2. range-scan the guild's prefix in each table, page by page, into the new cell
3. switch the placement (epoch + 1); writes resume on the new cell
4. drop the guild on the old cell, leaving a record that answers `moved`
```

Writes wait for as long as the copy takes, which is short for most guilds.
Very large guilds will want a copy while the guild keeps serving, then a
short freeze for what changed meanwhile (an SST export and a catch-up from
the log). PostgreSQL would need the same steps (export a guild's rows from
every table); a key prefix per guild makes it a range operation here. See
[Message Cell › Moving a guild](message-cell.md#moving-a-guild).

## Durability (the cost of embedding)

RocksDB keeps one disk's worth of durability and has no replication; that is
the real trade-off against a database server.

| | How | Lost on disk failure |
|---|---|---|
| Backups (done) | `BackupEngine` every few minutes, to another disk or object storage (`--backup-dir`) | writes since the last backup |
| Standbys (done) | `babel-standby` replays the primary's write-ahead log, batch by batch, on another machine | the last moment before failover |
| if needed | Raft per cell (e.g. openraft) | nothing, at 3× the cost |

A standby starts from a checkpoint of the primary and then applies every
write batch in order, so its sequence numbers match the primary's. It copies
the whole store, whichever service it belongs to; primaries keep six hours
of write-ahead log for standbys that fell behind. Failing over is manual
(see [deployment](../deployment.md#standbys)).

The Account service is the exception: relational, transactional, small, and
closed — it uses PostgreSQL in production.

## Crash safety

Every state change is one atomic batch with its log entry and, when other
services mirror it, its outbox entry; queued translation jobs are stored with
their full request. A process killed at any point resumes where it stopped
(`data_survives_a_crash` in the end-to-end tests kills the server and starts
it again).
