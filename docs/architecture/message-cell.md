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
| `member_left`, `member_removed` | Directory `remove_membership` |
| `message_created`, `message_edited` | Translation Cell `ingest` (revision = edit count; replies carry the message they answer) |
| `message_deleted` | Translation Cell `delete_message` |

Delivery is at least once; receivers are idempotent (`ingest` is keyed by
message and revision). An unavailable receiver stops the worker at that entry
so order is kept; a permanent rejection is logged and skipped so the outbox
never jams.

## Permissions

Roles are guild data, so permission checks live here. Phase 0 has two roles:

| | owner | member |
|---|---|---|
| view, send messages, start threads, invite | ✓ | ✓ |
| leave the guild | | ✓ |
| create categories and channels, change the language policy | ✓ | |
| remove (kick or ban) members, list bans, unban, see every invite | ✓ | |
| edit a message | own | own |
| delete a message | any | own |
| revoke an invite | any | own |

Other services ask through `check_permission` (the Edge does before changing
a guild's language policy on the Translation Cell).

## Channels

`category` > `text` > `thread`. A thread is created from a text channel and,
optionally, a starter message in it; the Translation Cell uses both to give
the thread context from its first message.

## Invites

Joining a guild takes an invite. Invites are guild data: they live in the
guild's cell, move with the guild and die with it, and no other service has
to know them.

- `create_invite` stores a random secret with an optional expiry and use
  limit; any member may invite.
- `join_guild(guild, secret, …)` checks and counts the invite in the same
  write that adds the member. Unknown, expired and used-up invites are all
  `not_found`, and so is an invite used by a banned user. A member joining
  again needs no valid invite.
- `get_invites` lists the valid invites (all for the owner, a member's own
  otherwise) and deletes expired and used-up ones on the way.
- The public code is `<guild id>-<secret>`, built by the Edge, so any Edge
  can route it to the right cell. The guild ID gives nothing away: without a
  valid secret it opens nothing.

## Leaving and removal

`leave_guild` and `remove_member` (owner only; the owner cannot leave or be
removed) delete the member, their read markers and notification settings
in one write with a `member_left` or `member_removed` log entry. Edges stop
fanning the guild out to that user when the entry reaches them; the outbox
removes the membership from the Directory. `remove_member` with `ban` also
stores a `Ban`, and a banned user's invites fail like invalid ones until
`unban_member`. Invites the member created stay valid.

## Mentions, replies and reading

- **Mentions.** `send_message` and `edit_message` resolve `@name` in the
  text against the guild's members: names are unique, case-insensitive and
  one word (the Account service refuses spaces and `@`), so the longest
  member name that ends where the word ends wins, and `mail@example` names
  no one. The message stores the member IDs (`mentions`, at most 20).
- **Replies.** `reply_to` names a message in the same channel; its author is
  mentioned too. The Translation Cell gets the reply relation as context.
- **Read markers.** `reads` keeps, per member and channel, the last message
  read and the number of later messages that mention them. A channel record
  keeps its newest message ID; a channel is unread while that is newer.
  Before a member has read anything, the marker is their join time (an ID
  made from the timestamp), so history from before they joined is not
  unread. The counts move in the same write as the message: `+1` for each
  mentioned member who has not read it, `−1` when an edit drops a mention or
  the message is deleted, and the author's marker jumps to their own
  message (writing reads the channel). `ack` moves a marker forward only,
  never past the newest message, and recounts the mentions after it from
  the messages themselves (at most 1000).
- **Notification settings** are stored per member (`notify`); the cell only
  keeps them. Clients decide what to show, since only they know which
  channel is on screen and what the message reads like in the reader's
  language.

## Storage

```
guilds    guild                       GuildRecord (guild, translation cell, last seq, last message id)
members   guild / user                Member
channels  guild / channel             Channel (with its newest message id)
messages  guild / channel / id        Message
invites   guild / secret              Invite (expiry, uses)
bans      guild / user                Ban
reads     guild / user / channel      ReadState (read marker, unread mentions)
notify    guild / user                NotificationSettings
log       guild / seq                 GuildEvent
outbox    seq                         OutboxEntry
```

Every key of a guild starts with its ID: moving a guild is a range export per
table. See [storage](storage.md).
