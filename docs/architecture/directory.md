# Directory

Code: `server/directory` · Contract: `types/directory.tyml`

The Directory is the global index. It is small and stays off the hot path:
Edges keep their own copy of its placement table.

## Placement

A placement says which Message Cell and which Translation Cell hold a guild:

```
Placement { guild_id, message_cell, translation_cell, epoch, version }
```

- `version` orders changes to the table. Edges follow it with
  `watch_placements(after)`.
- `epoch` increases when a guild moves; the old cell's writes are fenced off
  with it (migration is not implemented yet).

### Creating a guild

```
1. pick the least loaded Message Cell and Translation Cell
2. Message Cell.create_guild(random id, ...)    the guild exists, unreachable
3. commit the placement at version v + 1        now it is reachable
4. wake watchers; return the placement
```

The guild is created before its placement is committed: a crash in between
leaves an unreachable guild on a cell (to be garbage-collected), never a
placement pointing at nothing. Creation is serialized so versions are handed
out in order.

### Why Edges do not need to hear about it at once

See [Edge › Placement](edge.md#placement): the creating Edge learns from the
reply, the others from the watch, and any Edge asking about an unknown guild
falls back to `get_placement`. Nothing waits for every Edge to acknowledge.

## Membership index

`user → guilds`, used to build a user's guild list on connect. Message Cells
are the source of truth; they forward `member_joined` through their outbox.
The Edge double-checks with the Message Cell when it takes a snapshot, so a
stale entry only costs a skipped guild.

## Storage

```
placements   guild          Placement
versions     version        guild ID     (placement changes in order)
memberships  user / guild   ()
```

## Availability

When the Directory is down, existing guilds keep working: Edges route from
their copy. Only guild creation, migration and first lookups of guilds an Edge
has never seen stop. Phase 0 runs one instance; replication (e.g. Raft on its
small RocksDB) comes later.
