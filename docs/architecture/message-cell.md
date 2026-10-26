# Message Cell

Code: `server/message-cell` · Contract: `types/message_cell.tyml`

A Message Cell owns a set of guilds: their channels, members and original
messages. It is never reachable from the internet; Edges call it after
authenticating the user and resolving the guild's placement.

## Guilds have a single writer

All writes to a guild go through one writer (Phase 0: a per-guild lock; later
a per-guild actor task). Message order, IDs and permission checks need no
coordination beyond that.

Message IDs are time-ordered (milliseconds in the high bits, random low bits,
strictly increasing per guild) so history pages are range scans. They carry
no cell or worker number.

## One write, three consumers

Every change is one atomic batch:

```
state change + guild log entry (+ outbox entry)
     │                │                  └─▶ outbox worker ─▶ Translation Cell, Directory
     │                └─▶ poll_events ─▶ Edges ─▶ clients
     └─▶ reads: get_guild, get_messages
```

- **The guild log** (`log` table) is the guild's event history. Edges long-poll
  it (`poll_events`); `head` in each batch lets a follower start at the end.
  Later it will also drive migration catch-up and replication.
- **The outbox** (`outbox` table) holds pointers to log entries other services
  must mirror. A single worker delivers them in commit order, deleting each
  once delivered:

| Event | Forwarded as |
|---|---|
| `guild_created` | Translation Cell `put_space` |
| `channel_created` | Translation Cell `put_node` (threads carry their starter message) |
| `member_joined` | Directory `add_membership` |
| `message_created`, `message_edited` | Translation Cell `ingest` (revision = edit count) |
| `message_deleted` | Translation Cell `delete_message` |

Delivery is at least once; receivers are idempotent (`ingest` is keyed by
message and revision). An unavailable receiver stops the worker at that entry
so order is kept; a permanent rejection is logged and skipped so the outbox
never jams.

## Permissions

Roles are guild data, so permission checks live here. Phase 0 has two roles:

| | owner | member |
|---|---|---|
| view, send messages, start threads | ✓ | ✓ |
| create categories and channels, change the language policy | ✓ | |
| edit a message | own | own |
| delete a message | any | own |

Other services ask through `check_permission` (the Edge does before changing
a guild's language policy on the Translation Cell).

## Channels

`category` > `text` > `thread`. A thread is created from a text channel and,
optionally, a starter message in it; the Translation Cell uses both to give
the thread context from its first message.

## Storage

```
guilds    guild                       GuildRecord (guild, translation cell, last seq, last message id)
members   guild / user                Member
channels  guild / channel             Channel
messages  guild / channel / id        Message
log       guild / seq                 GuildEvent
outbox    seq                         OutboxEntry
```

Every key of a guild starts with its ID: moving a guild is a range export per
table. See [storage](storage.md).
