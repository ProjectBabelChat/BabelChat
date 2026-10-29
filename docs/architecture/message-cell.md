# Message Cell

Code: `server/message-cell` · Contract: `types/message_cell.tyml`

A Message Cell owns a set of guilds: their channels, members and original
messages. It is never reachable from the internet; Edges call it after
authenticating the user and resolving the guild's placement.

## Guilds have a single writer

All writes to a guild go through one writer (a per-guild lock; later perhaps
a per-guild actor task). Message order, IDs and permission checks need no
coordination beyond that.

Message IDs are time-ordered (milliseconds in the high bits, random low bits,
strictly increasing per guild) so history pages are range scans. They carry
no cell or worker number.

## One write, three consumers

Every change is one atomic batch:

```
state change + guild log entry (+ outbox entry)
     │                │                  └─▶ outbox worker ─▶ Translation Cell, Directory, Media
     │                └─▶ poll_events ─▶ Edges ─▶ clients
     └─▶ reads: get_guild, get_messages
```

- **The guild log** (`log` table) is the guild's event history. Edges long-poll
  it (`poll_events`); `head` in each batch lets a follower start at the end.
  Some entries concern one member: their read marker moved
  (`read_state_updated`) or their notification settings changed
  (`notifications_updated`). Edges pass those to that member's sessions
  only, so a user's devices stay in step on whichever Edge they are.
  `typing` entries (a member is writing in a channel) exist only in the
  log: they reach every Edge reading the guild and are trimmed with the
  rest, nothing else keeps them.
  Entries older than an hour are trimmed, except those the outbox has not
  delivered; `trimmed` in each batch tells a follower that fell further
  behind (its Edge then drops the sessions concerned, which reload).
- **The outbox** (`outbox` table) holds pointers to log entries other services
  must mirror. A single worker delivers them in commit order, deleting each
  once delivered:

| Event | Forwarded as |
|---|---|
| `guild_created`, `guild_updated` | Translation Cell `put_space` |
| `guild_deleted` | Translation Cell `delete_space`; every Media service `delete_guild` |
| `channel_created`, `channel_updated` | Translation Cell `put_node` (threads carry their starter message) |
| `channel_deleted` | Translation Cell `delete_node`; every Media service `delete_channel` |
| `member_joined` | Directory `add_membership` |
| `member_left`, `member_removed` | Directory `remove_membership` |
| `message_created`, `message_edited` | Translation Cell `ingest` (revision = edit count; replies carry the message they answer; not for a message of files alone); for a new message's files, Media `attach_blobs` |
| `message_deleted` | Translation Cell `delete_message`; its files, Media `delete_blobs` |
| `reaction_added`, `reaction_removed`, `typing`, `read_state_updated`, `notifications_updated` | nothing: only Edges read them |

Delivery is at least once; receivers are idempotent (`ingest` is keyed by
message and revision). An unavailable receiver stops the worker at that entry
so order is kept; a permanent rejection is logged and skipped so the outbox
never jams.

## Permissions

Roles are guild data, so permission checks live here. Phase 0 has two roles:

| | owner | member |
|---|---|---|
| view, send messages and files, react, start threads, invite | ✓ | ✓ |
| leave the guild | | ✓ |
| create, rename and delete categories and channels; change the language policy | ✓ | |
| rename and delete a thread | any | own |
| rename the guild, delete it | ✓ | |
| remove (kick or ban) members, list bans, unban, see every invite | ✓ | |
| edit a message | own | own |
| delete a message | any | own |
| revoke an invite | any | own |

Other services ask through `check_permission` (the Edge does before changing
a guild's language policy on the Translation Cell).

## Channels

`category` > `text` > `thread`. A thread is created from a text channel and,
optionally, a starter message in it; the Translation Cell uses both to give
the thread context from its first message. Channels remember who made them
(`created_by`; absent on older ones), which lets a thread's creator manage
it.

- **Renaming** (`update_channel`) changes the name and topic; the outbox
  passes the new title to the Translation Cell's node, whose thread fork
  stays as it was.
- **Deleting** (`delete_channel`) takes a text channel's threads first,
  then the channel, each with its messages and every member's read marker
  for it, and one `channel_deleted` entry each; the Translation Cell drops
  their nodes, windows and translations. A deleted category's channels stay,
  moved out of it (`channel_updated`).

## Reactions

A message keeps its reactions (`reactions`: each emoji with the members who
reacted, in order, at most 20 emoji). Reacting is not editing: `rev`,
`edited_at` and the translations stay, and nothing goes to the Translation
Cell. Each change is one `reaction_added` or `reaction_removed` entry
naming the member and the emoji's new count, so applying it twice does no
harm. Emoji are Unicode emoji in their fully qualified form (`❤` is stored
as `❤️`), checked against the Unicode list; anything else is
`invalid_request`. A guild's own emoji would be `custom:<id>`, usable in
other guilds too, once images can be uploaded; nothing refers to them yet.

## Attachments

Files themselves are on a [Media](media.md) service; a message keeps
their records (`attachments`: ID, name, type, size, image size).

- **Uploads.** The Edge stores a file on a Media service, then records it
  here (`add_upload`) for the member, who must be allowed to write in the
  channel: `uploads` holds it under the member for a day (at most 50 per
  member and guild). Older ones are forgotten with the log trimming; the
  Media service deletes their files on its own.
- **Sending** (`send_message` with `attachments`) takes up to 10 of the
  member's uploads to that channel, in the same write as the message, so
  each goes with one message only. With files, the text may be empty;
  such a message is not sent to the Translation Cell.
- **Edits** keep the files. Deleting a message lists its files in the
  `message_deleted` entry (`DeletedMessage.attachments`) for the outbox to
  delete; deleting a channel or a guild deletes theirs on every Media
  service.

## Deleting a guild

`delete_guild` (owner only) drops everything the guild holds in one write
but its record and its log, marks the record deleted and appends
`guild_deleted`, then a `member_removed` for every member. Edges read the
log to its end (polling past it answers `not_found`) and forget the guild;
the outbox removes the memberships from the Directory and the space from
the Translation Cell. Every other call about the guild answers
`not_found`. The record stays, and the log until it is trimmed; the
Directory keeps the placement, which leads nowhere.

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
  keeps them, and logs changes for the member's other sessions. Clients decide what to show, since only they know which
  channel is on screen and what the message reads like in the reader's
  language.

## Storage

```
guilds    guild                       GuildRecord (guild, translation cell, last seq, last message id,
                                      trimmed up to, frozen, moved, deleted)
members   guild / user                Member
channels  guild / channel             Channel (with its newest message id and creator)
messages  guild / channel / id        Message (with its reactions and attachments)
invites   guild / secret              Invite (expiry, uses)
bans      guild / user                Ban
reads     guild / user / channel      ReadState (read marker, unread mentions)
notify    guild / user                NotificationSettings
uploads   guild / user / attachment    Upload (a file not sent yet; a day)
log       guild / seq                 GuildEvent
outbox    seq                         OutboxEntry
```

Every key of a guild starts with its ID: moving a guild is a range export per
table. See [storage](storage.md).

## Moving a guild

The Directory moves a guild between cells (`babel-admin move-guild`;
`moving.rs` on the cells' side):

```
Directory          old cell                        new cell
   │ discard ──────────────────────────────────────▶ leftovers of a failed move go
   │ freeze ────────▶ writes wait; the outbox drains
   │ export ────────▶ pages of entries ──▶ import ──▶ stored, still frozen
   │ placement switches (epoch + 1); Edges follow
   │ finish(moved: false) ─────────────────────────▶ writes go on here
   │ finish(moved: true) ─▶ data dropped; waiting
   │                        writers hear `moved`
```

- **Writes wait, reads go on.** A frozen guild holds writers (up to 30
  seconds) instead of failing them; reads and the log stay available on the
  old cell until the move ends.
- **Entries move as stored**, guild record last, so the log keeps its
  sequence numbers and message IDs keep increasing: Edges carry on
  following the log from where they were.
- **The outbox drains first**: entries left for the Translation Cell or the
  Directory leave from the old cell, in order, before the copy starts.
- **`moved`.** Afterwards the old cell keeps only the guild's record, and
  every call about the guild answers `moved`. Edges then look the guild up
  again and repeat the call on the new cell, so clients notice nothing.
- A move that fails before the placement switches is undone: the copy is
  discarded and writes resume on the old cell.
