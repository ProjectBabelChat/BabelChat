# Client SDK

`client/sdk` (`babel-sdk`) is everything a BabelChat app needs except the
UI: signing in, a live session, local state, which text to show each
message in, and on-device translation. The terminal client (`client/cli`)
is built on it, and so is the [desktop app](desktop.md) (GPUI + MD3E).

```
 app (terminal, desktop)
   │  calls                         ▲ Update: what changed
   ▼                                │
 Session ── state      guilds, channels, members, messages, translations
   │        display    which text each message shows this reader
   │        device     on-device translation, through English
   ▼
 Edge   requests (generated client, one connection pool)
        + gateway events over a WebSocket, or long polls
```

## Using it

```rust
let client = Client::login("https://edge.example", "alice", password).await?;
let config = SessionConfig {
    cache: Some(data_dir.join("alice.json")),
    ..SessionConfig::default()
};
let session = client
    .connect_with(Arc::new(MockDeviceTranslator::default()), config)
    .await?;
let mut updates = session.updates();

let channel = &session.guilds()[0].channels[0].id;
session.focus(Some(channel));
session.load_history(channel, 50).await?;
session.send(channel, "@bob 明日の夜、一緒に遊びませんか").await?;

while let Ok(update) = updates.recv().await {
    match update {
        Update::Notification(n) => notify_os(&n.message.display.text),
        // e.g. Update::Message { channel_id, message_id, change: Translated, .. }
        _ => redraw(&session),
    }
}
```

The app reads state from the session (`guilds`, `messages`, `message`,
`settings`) and redraws when an `Update` names what changed. Updates are
notifications, not data: the session is always the source of truth, so a
receiver that lags (`RecvError::Lagged`) just redraws everything.

| Update | When |
|---|---|
| `Guild { change: Joined / Left / Removed / MemberJoined / MemberLeft / MemberRemoved / ChannelCreated / Languages / Notifications / Refreshed }` | guild list or a guild's view changed (`Removed`: this user was kicked or banned) |
| `Message { change: Created / Edited / Translated / Deleted }` | a message or its display changed (`Translated` covers server and device results) |
| `Unread { channel_id }` | the channel's unread marker or mention count changed |
| `Notification(Notification)` | something the user wants to hear about, with its text in their language |
| `Offline { error }` / `Online { reloaded }` | connection lost / back; with `reloaded`, redraw from scratch |
| `SignedOut` | the token was rejected; the session stopped |

## Which text a reader sees

Each message comes with a `Display { text, shown }`, decided in
`display.rs` from the message, its translations and the reader's
`LanguageView` (computed by the Edge):

1. **Original** — the reader's own message, or already in their language.
2. **Server** — the LLM translation into the reader's language.
3. **Device** — translated on this device: from the server's English
   translation when there is one (one step, and it carries the LLM's
   context), otherwise from the original through English (two steps).
4. **Untranslated** — the original, because nothing else is available.

`Translating` shows the original while a translation is on its way.
Translations of an older revision are ignored, so an edit shows its new
text at once and its new translation when it arrives.

Who ends up where:

| Reader | Guild | Shown |
|---|---|---|
| in an LLM language | paid | Server |
| in the guild's languages, not LLM | paid | Device, from English |
| outside the guild's languages, `translate_outside_set` on | any | Device |
| outside, `translate_outside_set` off | any | Original |
| any | free | Device (N → en → N) |
| LLM language, but the LLM failed or ran out of budget | paid | Device, as a fallback |

## On-device translation

`DeviceTranslator` is the seam for local models:

```rust
#[async_trait]
pub trait DeviceTranslator: Send + Sync {
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<String, DeviceError>;
}
```

Implementations are only asked for pairs with English on one side, which is
how Bergamot's models come; the SDK chains them (`device.rs`). Jobs run in
the background, two at a time, keyed by message, revision and language
pair, and their results are cached. A job that failed in a way that may
pass (`DeviceError::Failed`: a download cut short) is tried again when the
message is looked at 30 seconds later or more; "no model for this language"
(`Unsupported`) is final. `MockDeviceTranslator` prefixes the target tag
(`[device fr-FR] …`) for tests.

### Real models: `babel-bergamot`

`client/bergamot` implements `DeviceTranslator` with the Firefox
Translations models, the descendants of Project Bergamot: one small int8
CPU model per direction, English on one side, about 100 directions.
[fxtranslate](https://crates.io/crates/fxtranslate) runs them in Rust (a
port of Firefox's engine that uses the same gemmology kernels; a small C++
SIMD shim, with a pure-Rust fallback) and finds, downloads and verifies
them.

```rust
let translator = BergamotTranslator::new(BergamotConfig {
    models_dir: cache_dir.join("models"),
    download: true,
});
let session = client.connect(Arc::new(translator)).await?;
```

- A direction's model (35–55 MB on disk) is downloaded on its first use
  and kept in `models_dir`; a model on disk is used without any request.
  Several processes may share the directory: a file lock makes one download
  while the others wait for it.
- Engines may be shared between threads but must be created and dropped on
  one, so a single model thread owns them: it loads each direction once and
  runs every translation. On this development machine a sentence takes
  150–200 ms, loading included.
- Tags map to model names by language (`ja-JP` → `ja`), except Chinese,
  whose models are per script (`zh-TW` → `zh-Hant`).
- fxtranslate 0.5 leaves SentencePiece byte pieces in its output
  (`<0xC3><0x87>a` for `Ça`); the crate turns them back into text.

The models are discovered through Firefox's Remote Settings and served from
Firefox's CDN, which are provisioned for Firefox. A released app must
mirror the models it needs and serve them itself; fxtranslate takes another
records URL from `FXTRANSLATE_RECORDS_URL`, and the files can be put in
`models_dir` directly.

## Saved sign-ins

`babel_sdk::profile` keeps one sign-in per profile name in
`~/.config/babelchat/<profile>.json` (mode 0600), with the session cache
next to it (`<profile>.cache.json`). Every app on the machine shares them:
a profile signed in from the terminal client opens in the desktop app, and
the other way round.

A token changes when the user signs the other devices out or changes the
password (`Client::sign_out_others`, `Client::change_password`): apps save
the new one. The session carries on with it by itself; a session refused
while its token was being replaced retries instead of signing out.

## Unread channels and notifications

The server keeps, per member and channel, a read marker and the number of
unread messages that mention them (`ReadState`), and each channel's newest
message ID. A channel is unread while its newest message is newer than the
marker (IDs compare as numbers). `Session::unread(channel)` and
`guild_unread(guild)` answer from local state, which follows the server's
rules as events arrive: a new message moves the channel's newest ID,
writing reads the channel, and mentions of this user count until read.

- `mark_read(channel)` acks the newest message; the server tells the user's
  other sessions (`read_state_updated`), so reading on the phone clears the
  desktop.
- `focus(Some(channel))` names the channel on screen. It is marked read at
  once and as it grows (`read_delay`, 500 ms, batches a burst into one
  request), and it never notifies.
- Mentions are `@name` in the text; the server resolves them against the
  guild's members (names are one word and unique). A reply mentions the
  author of the message it answers. `Session::reply` sends one.

A new message from someone else notifies when the user's settings for its
channel say so: a channel's own entry, else its parent's (a thread follows
its channel, a channel its category), else the guild's level — `all`
(default), `mentions` (mentions and replies) or `none`. Settings live on the
server (`set_notifications`) and follow the user to other devices.

`Update::Notification` waits until the message's display has settled, up
to `notify_wait` (3 s), so it carries the text in the reader's language:
apps can hand it to the OS as it is. Its `reason` is `Mention`, `Reply` or
`Message`. Notifications come from live sessions only; push notifications
to devices that are not connected need a push service (later).

## Staying connected

```
connect ─▶ Ready (guilds) ─▶ events: WebSocket (or long polls) ─▶ state ─▶ Update
                                │
              network error ────┤ resume the same session with backoff
                                │ (the Edge keeps its queue for 10 minutes)
              session gone ─────┘ connect again; reload guilds and the
                                  newest page of every opened channel
```

Events arrive over a WebSocket (`GET /gateway/ws`) by default. The socket
carries the same `EventBatch`es as long polls, from the same per-session
queue, so the session resumes from its cursor either way. `Transport::Auto`
falls back to long polling when the server will not upgrade;
`Transport::LongPoll` and `Transport::WebSocket` force one.

The Edge answers `not_found` to a poll when it no longer has the session:
after a restart, after ten idle minutes, or when the client fell further
behind than the session's queue (1000 events). The SDK then reconnects and
reloads; messages deleted while it was away disappear because the reloaded
page is authoritative for its range.

Events can arrive twice (the Edge subscribes before taking a snapshot) and a
translation can arrive before its message (the Edge follows the two logs
separately). State is keyed by message ID and revision, so duplicates are
dropped and early translations wait for their message.

## The cache

With `SessionConfig::cache` set to a file, the session keeps its state
there: guilds (with read markers and notification settings), the newest 100
messages of every channel it has seen, their translations, and the device
translations of those messages, so they are not translated again. The file
is written every `cache_interval` (5 s) while something changes, and when
the session is dropped; it is replaced whole (temporary file + rename) and
readable only by its owner (mode 0600).

A session with a cache starts from it at once, offline too, and connects in
the background exactly as it reconnects: the server's guilds replace the
cached ones and every cached channel's newest page is reloaded,
authoritatively. The cache is never the truth, only a head start; a file
for another user or server, or of another format version, is ignored.

## Runtimes

The SDK runs on Tokio, because the generated client uses reqwest. An app on
another executor (GPUI) starts a Tokio runtime on a background thread and
connects from inside it. After that, the synchronous `Session` methods
(state reads) can be called from any thread. The async ones must run on
that runtime, because reqwest's I/O needs Tokio's reactor: spawn them with
the runtime's `Handle` and await the `JoinHandle`, which works from any
executor. Device jobs already run on the runtime the session was connected
on.

## Not yet

- Mirroring the device models, and updating them when new versions ship.
- Push notifications for devices that are not connected.
- Typing indicators.
