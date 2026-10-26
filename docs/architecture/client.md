# Client SDK

`client/sdk` (`babel-sdk`) is everything a BabelChat app needs except the
UI: signing in, a live session, local state, which text to show each
message in, and on-device translation. The terminal client (`client/cli`)
is built on it, and the desktop app (GPUI + MD3E) will be too.

```
 app (CLI today, GPUI later)
   │  calls                         ▲ Update: what changed
   ▼                                │
 Session ── state      guilds, channels, members, messages, translations
   │        display    which text each message shows this reader
   │        device     on-device translation, through English
   ▼
 Edge   requests + long-poll gateway (generated client, one connection pool)
```

## Using it

```rust
let client = Client::login("https://edge.example", "alice", password).await?;
let session = client.connect(Arc::new(MockDeviceTranslator::default())).await?;
let mut updates = session.updates();

let channel = &session.guilds()[0].channels[0].id;
session.load_history(channel, 50).await?;
session.send(channel, "明日の夜、一緒に遊びませんか").await?;

while let Ok(update) = updates.recv().await {
    // e.g. Update::Message { channel_id, message_id, change: Translated, .. }
    for message in session.messages(channel) {
        println!("{}: {}", message.message.author_name, message.display.text);
    }
}
```

The app reads state from the session (`guilds`, `messages`, `message`,
`settings`) and redraws when an `Update` names what changed. Updates are
notifications, not data: the session is always the source of truth, so a
receiver that lags (`RecvError::Lagged`) just redraws everything.

| Update | When |
|---|---|
| `Guild { change: Joined / Left / MemberJoined / ChannelCreated / Languages / Refreshed }` | guild list or a guild's view changed |
| `Message { change: Created / Edited / Translated / Deleted }` | a message or its display changed (`Translated` covers server and device results) |
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
pair, and their results are cached. `MockDeviceTranslator` prefixes the
target tag (`[device fr-FR] …`) until real models are wired in.

## Staying connected

```
connect ─▶ Ready (guilds) ─▶ poll loop ─▶ events ─▶ state ─▶ Update
                                │
              network error ────┤ retry the same session with backoff
                                │ (the Edge keeps its queue for 10 minutes)
              session gone ─────┘ connect again; reload guilds and the
                                  newest page of every opened channel
```

The Edge answers `not_found` to a poll when it no longer has the session:
after a restart, after ten idle minutes, or when the client fell further
behind than the session's queue (1000 events). The SDK then reconnects and
reloads; messages deleted while it was away disappear because the reloaded
page is authoritative for its range.

Events can arrive twice (the Edge subscribes before taking a snapshot) and a
translation can arrive before its message (the Edge follows the two logs
separately). State is keyed by message ID and revision, so duplicates are
dropped and early translations wait for their message.

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

- Real device models (Bergamot) and their downloads.
- Persisting state between runs; today a session starts from the server.
- A WebSocket transport (the SDK API will not change).
- Typing indicators, read state, notifications.
