# Edge

Code: `server/edge` · Contract: `types/edge.tyml`

The Edge is the only server clients talk to, and the only one with a public
address. It holds nothing that cannot be rebuilt, so Edges can be added,
removed or replaced while traffic flows.

## Responsibilities

| | How |
|---|---|
| Authentication | JWTs (EdDSA) verified locally with the Account service's public key; no call on the request path |
| Rate limiting | Token bucket per user on writes, acks included (`RateLimiter`) |
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

A Message Cell answers `moved` for a guild that left it. The Edge then looks
the placement up again and repeats the call on the new cell (`on_cell!` in
`lib.rs`); its log followers carry on from the same sequence number there.
Writes that arrive while a guild is being copied wait at the cell, so a
move costs clients a pause, not an error.

## Gateway

`gateway.rs`, `websocket.rs` and the `follow_*` functions in `lib.rs`.

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
- Queues are delivered over a WebSocket (`GET /gateway/ws?session_id=..&after=..`,
  `websocket.rs`) or by long polling (`Gateway.poll`). The socket pushes
  each `EventBatch` as a JSON text frame as soon as events exist, and pings
  when there are none for 25 seconds; the client sends nothing. Both drain
  the same queue, so a client can switch between them and resume from its
  cursor. Close code 4404 means the session is gone. The route sits next to
  the generated router (TYML describes request/response functions; the
  socket only carries the generated `EventBatch` type).
- A session follows a guild from the snapshot it started from. When a
  member leaves or is removed, the guild's log says so; every Edge passes
  the event on (to the member's own sessions too, as their last event from
  the guild) and then drops the member's subscriptions made from older
  snapshots, so someone who joined again keeps theirs. A snapshot the cell
  refuses (the Directory's copy of a membership lags) undoes the
  subscription it was taken for.
- Some events concern one user rather than a guild: a read marker moved
  (`read_state_updated`) or notification settings changed
  (`notifications_updated`). The Message Cell logs them in the guild's log
  with the user's ID, and every Edge passes them to that user's sessions
  only, so the user's devices follow wherever they are connected.
- A guild joined (or created) through one Edge reaches the user's sessions
  on other Edges through the Directory's membership stream
  (`watch_memberships`): each Edge follows it from its start, and
  subscribes a user's sessions that do not read the guild yet, with a
  `guild_joined` event.
- Logs are trimmed after an hour. A follower told that its log was trimmed
  past its cursor drops the sessions of that guild (or all sessions, for
  the membership stream); their clients reload.
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

## Running apart

`babel-edge` serves the public API in front of a cluster (see
[deployment](../deployment.md)); it reaches every other service through
the `Remote*` clients of `babel-net`, over mutual TLS, and verifies tokens
with the Account service's Ed25519 public key. A client's gateway session
lives on one Edge, so load balancers keep each client on one Edge.

## Not yet

- Per-IP limits on login and registration (the generated router does not
  expose the client address yet).
- Settings changes reaching a user's sessions on other Edges (they apply
  there on the next connect).
