# Media

Code: `server/media` · Contract: `types/media.tyml`

The Media service keeps the files people upload: attachments of messages
and a guild's own emoji. Like a cell, it is never reachable from the
internet; Edges pass files to it after checking who may send or see them,
and Message Cells tell it which files messages took in and which went.

## Files in an embedded store

Files live in the service's RocksDB store, like every other service's data,
so standbys (the write-ahead log, replayed), backups, restores and failover
work as for every store, and a machine needs nothing but SSH. What makes
this fit large values is RocksDB's integrated BlobDB: the `blobs` table
keeps values from 64 KB up in blob files beside the LSM tree, the
key-value separation of WiscKey (as Facebook's photo storage keeps photos
in large append-only files with an index). Compactions move only the small
pointers; the files themselves are written once.

```
table     key                                    value
blobs     guild / channel / id / chunk           1 MB of the file (blob files)
meta      guild / channel / id                   Blob: name, type, size, image size,
                                                 uploader, uploaded at, attached at
by_time   uploaded at / guild / channel / id     BlobRef   (every file, oldest first)
pending   uploaded at / guild / channel / id     BlobRef   (files no message took in yet)
```

- **Chunks** of 1 MB: an upload is stored as it arrives and a download is
  read one chunk at a time, so neither is held in memory whole.
- **Keys start with the guild and the channel**, so deleting a channel's or
  a guild's files is one range deletion. Their entries in the time indexes
  stay behind and are dropped when next met.
- **Blob files are uncompressed** (images and archives already are), and
  their space comes back through blob garbage collection during
  compactions, some time after the files are deleted.
- A standby opens its copy with the options the primary's tables have
  (`RocksStore::open_existing` reads them from the checkpoint), blob files
  included.

## IDs say where a file is

A file's ID is `<media service>.<random>` (`media-0.3f9c…`). Anyone holding
one knows which Media service to ask, without the Directory, as invite
codes carry their guild. New files go to one Media service per guild (a
hash of the guild among the services), so a guild's files tend to stay
together; adding a Media service only changes where new files go.

## Uploading, sending, deleting

```
client ─ POST /media/upload ─▶ Edge ─ check_permission(send) ─▶ Message Cell
                                     ─ PUT /media/blobs (streamed) ─▶ Media: stored, pending
                                     ─ add_upload ─▶ Message Cell: the member's upload (a day)
client ─ send_message(attachments: [id]) ─▶ Edge ─▶ Message Cell: takes the uploads,
                                                     the message keeps them
                     outbox: message_created ─ attach_blobs ─▶ Media: no longer pending
                     outbox: message_deleted ─ delete_blobs ─▶ Media
                     outbox: channel_deleted ─ delete_channel ─▶ every Media service
                     outbox: guild_deleted ─ delete_guild ─▶ every Media service
```

- An upload is the member's for a day: only they can send it, only in the
  channel it was uploaded to, and only once. At most 10 go with a message,
  whose text may then be empty (nothing goes to the Translation Cell then).
  A member may have 50 waiting per guild.
- Files nothing took in are deleted by the Media service itself after two
  days (`pending`), whatever happened in between: an Edge that stopped
  between storing a file and recording the upload leaves nothing behind
  for long.
- Edits keep a message's files. Deleting the message, its channel or its
  guild deletes them.

## Reading files

`GET /media/<guild>/<channel>/<id>` on any Edge, with the user's token. The
Edge checks that the user may see the guild (asking its cell at most once a
minute per user and guild), then streams the file from its Media service.

- The type is read from the file's first bytes by the Media service, never
  taken from the uploader (`sniff.rs`). PNG, JPEG, GIF and WebP come back
  as themselves (`Content-Disposition: inline`); anything else as
  `application/octet-stream`, a download. Every answer carries
  `X-Content-Type-Options: nosniff`, so a file can never act as a page.
- Images are measured from their header at upload (`width`, `height`), so
  clients lay messages out before the image arrives.
- A file never changes under its ID: answers may be cached for good
  (`Cache-Control: private, max-age=31536000, immutable`), and clients keep
  downloaded files by ID (the SDK's media cache).

## A guild's emoji

A guild's emoji are blobs of kind `emoji`, of no channel (keys hold an
empty channel). Their guild's members use them in any guild, so their
images are for anyone signed in (`GET /media/<guild>/-/<id>` on an Edge),
while attachments stay with the guild's members; the record's `kind`
decides which rule applies, and maintenance can tell them apart.

```
client ─ POST /media/emoji?guild_id=&name= ─▶ Edge ─ check_permission(manage) ─▶ Message Cell
                                               ─ PUT /media/blobs (kind emoji) ─▶ Media: pending
                                               ─ create_emoji ─▶ Message Cell: the guild's emoji
                        outbox: emoji_created ─ attach_blobs ─▶ Media: no longer pending
                        outbox: emoji_deleted ─ delete_blobs ─▶ Media
```

- An emoji's image is a PNG, JPEG, GIF or WebP of up to 256 KB; the Media
  service refuses more as it arrives, the Edge anything that is not a
  picture.
- The emoji itself (name, image ID, who added it) is kept by its guild's
  Message Cell; see [Message Cell](message-cell.md#a-guilds-emoji) for how
  text and reactions name it, and [Edge](edge.md#a-guilds-emoji) for who
  may use it where.
- Deleting the guild deletes its emoji's images with its other files.

## Maintenance

Every file records when it was uploaded and when a message took it in, and
`by_time` walks every file oldest first: `list_blobs(before, kind)` pages
through files uploaded before a time (attachments only, say), and
`delete_blobs` removes them, for a program that frees space by age.
`get_usage` counts files and bytes (`babel-admin media-usage`).

## Limits

- 10 MB per file, 256 KB per emoji (`limit_exceeded`), checked against
  `Content-Length` by the Edge and while the file arrives by the Media
  service.
- Names are kept as given, without path or control characters, at most 255
  characters.

## Not yet

- Object storage (S3) behind the same API, for clusters that would rather
  not keep files on their own disks.
- Thumbnails, and ranges of a file (video).
- Quotas per user or guild; scanning files.
