# Edge

Code: `server/edge` · Contract: `types/edge.tyml`

The Edge is the only server clients talk to, and the only one with a public
address. It holds nothing that cannot be rebuilt, so Edges can be added,
removed or replaced while traffic flows.

## Responsibilities

| | How |
|---|---|
| Authentication | JWTs verified locally with the Account service's key; no call on the request path |
| Rate limiting | Token bucket per user on writes (`RateLimiter`) |
| Placement | Resolves guild → cells from its copy of the Directory's table ([placement](#placement)) |
| Joining answers | e.g. `get_messages` = Message Cell history + Translation Cell translations for the reader |
| Permissions handoff | Asks the Message Cell (`check_permission`) before calling the Translation Cell on a user's behalf |
| Invites | Builds public invite codes (`<guild id>-<secret>`) and routes them back to the guild's cell, which checks them ([Message Cell](message-cell.md#invites)) |
| Fan-out | Follows each guild once and copies events into every interested session ([gateway](#gateway)) |
| The public boundary | Converts internal types to public ones (`convert.rs`) |

## The public boundary

Public types are generated from `types/edge.tyml`, separately from internal
types. Internal fields (cell names, epochs, an author's locale) cannot reach a
client by accident: every crossing is written out in `convert.rs`.

Errors are folded on the way out. `not_found` and `forbidden` from a Message
Cell both become `not_found`, so a guild ID cannot be probed for existence.

## Placement

`placement.rs` keeps a full copy of the placement table:

1. **insert** — the Edge that relayed `create_guild` stores the placement from
   the reply, so the creator's next request works immediately;
2. **watch** — a long poll on `Directory.watch_placements` streams every
   committed placement;
3. **resolve on a miss** — an unknown guild is looked up with
   `get_placement`. Concurrent misses for one guild share one lookup, and
   misses are remembered for five seconds, so floods of random IDs do not
   reach the Directory.

Placements carry a version; an older one never overwrites a newer one, which
matters once guilds move between cells.

## Gateway

`gateway.rs` and the `follow_*` functions in `lib.rs`.

```
Message Cell log ──┐                 ┌─▶ session A queue (alice, ja-JP)
                   ├─▶ Edge follower ┼─▶ session B queue (bob, en-GB)
Translation log ───┘   (one per guild)└─▶ session C queue (carol, fr-FR)
```

- An Edge follows each guild its sessions read exactly once: one long poll on
  the Message Cell log, one on the Translation Cell log. Cells send each event
  to each Edge once, however many users read it.
- Translations are filtered per session: the reader's effective language plus
  the guild's English (the pivot for on-device translation).
- A session's guilds are registered **before** its snapshot is taken, so no
  event falls between the snapshot and the stream. The cost is an occasional
  duplicate, which clients drop by message ID.
- Queues are delivered by long polling (`Gateway.poll`). A WebSocket
  transport will drain the same queues with the same `Event` type.
- A queue keeps the last 1000 events, so a client that briefly lost its
  connection polls on from where it was. Sessions not polled for ten
  minutes are dropped, and so is a session asked for events it has already
  pushed out of its queue. Either way the poll answers `not_found` and the
  client connects again and reloads (the SDK does this on its own; see
  [client SDK](client.md)).

## Language views

The Edge resolves how each user reads each guild (`LanguageView`) from the
user's settings (Account) and the guild's policy (Translation Cell), using
`babel-lang`. Clients receive the result and need no normalization tables.
Policy changes arrive as `policy_updated` events with a fresh view per user.

## Not yet

- WebSocket transport.
- Per-IP limits on login and registration (the generated router does not
  expose the client address yet).
- HTTP clients for split deployments (in-process only in Phase 0).
