# TYML changes made for BabelChat

BabelChat uses a local TYML checkout (`../tyml`) while these changes are
reviewed. They follow two rules:

1. TYML stays minimal and covers 90% of cases.
2. When an existing feature can express something without costing
   performance or safety, use it instead of adding a feature.

**No language feature was added.** All changes are in the code generator
(`src/tyml_api_generator`), are uncommitted in the TYML repository, and the
generator's tests pass (20 tests, 3 of them new).

## Fixes to generated code

| Problem | Effect before | Now |
|---|---|---|
| Struct fields came out in hash-map order | Field order changed on every generation (Rust, TypeScript, Kotlin), so regenerating produced noisy diffs | Declaration order (`general::ordered_elements`), in all three generators |
| Enum literals became Rust identifiers verbatim | `"en-US"` did not compile; `"merged"` produced a lowercase variant | `UpperCamelCase` variants with `#[serde(rename = "...")]`; clashes get a numeric suffix |
| Map types (`{ *: T }`) became empty structs | Map contents were silently dropped on deserialization | `BTreeMap<String, T>`; a named map becomes a type alias; fixed fields plus `*` flatten the rest into `__rest` |
| Field names that are Rust keywords or not identifiers | `ref: string` or `"display-name": string` did not compile | Raw identifiers (`r#ref`) or a sanitized name with `#[serde(rename)]` |
| Recursive named types | Marked as generated only after generating them, so a type referring to itself recursed without end | Marked before generating |
| `JwtValidator::validate` took no `&self` | The signing key had to live in a global | `fn validate<T>(&self, token: &str)`; the router passes the API value |
| The Rust client's `ThrowsOrOtherError` had no `Debug` | Results could not be `unwrap`ped or printed | `#[derive(Debug)]` |

The `JwtValidator` change breaks existing implementations: they need a
`&self` parameter.

## Small additions to generated code

- Structs derive `PartialEq`; enums also derive `Eq` and `Hash` (handy in
  tests and maps).
- Generated crates start with `#![allow(clippy::all)]`, so a workspace-wide
  clippy run does not lint generated code.
- Doc comments are emitted one per line regardless of trailing newlines.

## Expressed with existing features instead

| Need | How |
|---|---|
| Events with different payloads | A `kind` enum and one optional field per payload |
| Streams (event logs, placement watch) | Long-poll functions: `poll_events(after, wait_ms) -> { events, next, head }` |
| Sharing types across services | One file per boundary; a client crate converts to the server crate's types with serde (`babel_common::convert`) |
| Ordered key-value settings | Arrays of pairs (`[LanguageAlias]`) |
| Timestamps | `int` milliseconds |

## Left as is, for later

- Every thrown error is HTTP 400 (401 for bad tokens). Fine while clients
  are generated; worth revisiting for third-party SDK users.
- No `import`: one file per boundary works for now; a few concepts
  (`Translation`) exist in two files and are converted explicitly.
- The generated router does not expose the client address, which per-IP
  rate limiting on login will need.
- The enum, map and keyword fixes are in the Rust generator only; the
  TypeScript and Kotlin generators only got the field-order fix.

## Phase 1 (chat without a UI)

No TYML changes. The SDK needed one shared connection pool for all
interfaces; the generated clients already expose `url` and `client` as
public fields, so it builds them directly instead of calling `new`.

## Phase 2 (chat basics)

No TYML changes. Two needs were met with existing features:

| Need | How |
|---|---|
| A WebSocket gateway | TYML describes request/response functions. The socket is a hand-written Axum route next to the generated router; it carries the generated `EventBatch` type, and its protocol is written in the `Gateway` interface's doc comment |
| Adding a field to a stored type without breaking older data | The field is optional (`mentions: [string]?` in the Message Cell's `Message`); the public type keeps it required and the Edge fills in an empty list |

TYML's declared default values (`field: T = value`) would express the
second more directly, but the Rust API generator ignores them today (no
`#[serde(default)]`). Worth adding when a required stored field must be
added again.

## Phase 3 (the desktop app)

No TYML changes. The desktop app uses only the SDK, which already covered
everything it shows.

## Phase 4 (split it up)

No TYML changes. The generator already produced everything splitting the
services up needed:

| Need | How |
|---|---|
| HTTP clients for the internal APIs | The client generator, run on three more files (`account`, `directory`, `message_cell`); `babel-net` puts the clients behind the server crates' traits with a small macro |
| Mutual TLS for the clients | The generated clients expose `url` and `client`, so they take a `reqwest::Client` set up with the service's certificate |
| Mutual TLS and caller identity for the servers | The generated `router(api)` is an Axum router; `babel-net` serves it over TLS and tells handlers who calls through a task-local, so no function needed a "caller" parameter |
| Long polls that say the log was trimmed | A `trimmed: int` field in the existing batch types |
| A request body that is a list | A wrapper type (`GuildImport { guild_id, entries }`) |
| Binary keys and values when moving a guild | Hex strings |
| Replication (binary write batches, file streams) | Hand-written routes next to the generated router, like the WebSocket gateway |

Functions that could not fail before (the Directory's membership calls and
placement watch) now `throw` like the others: over a network, every call
can.

## Phase 5 (real translation, choosing models)

No TYML changes. Choosing a model per space needed:

| Need | How |
|---|---|
| A field added to types that are stored (`SpacePolicy.model`, `TranslateRequest.model` in queued jobs) | Optional fields (`model: string?`): records written before read as null, which means "the default model" |
| An engine listing several models | `EngineInfo { models: [ModelInfo] }`, replacing its two fields; nothing outside the repository calls the Engine API yet |
| A request naming a model the engine does not serve | One more `EngineError` variant, `unknown_model` |

## Phase 6 (running it for real)

No TYML changes. An open server's account basics needed:

| Need | How |
|---|---|
| A sign-up code in registrations | An optional field (`signup_code: string?`) in both `Registration` types: clients that send none still work where sign-ups are open |
| Changing the password, signing other devices out | Functions returning a new `Token` for the device asking (`change_password`, `sign_out_others`), in the Account API and the Edge's `Me` |
| Making sign-up codes (operators only) | `create_signup_code`, kept from Edges by the service's policy like other operator functions |
| Edges refusing revoked tokens without calling the Account service per request | A long-polled stream like the Directory's (`watch_revocations` with `Revocation` and `RevocationBatch`); no `trimmed` field, since revocations only matter while the tokens they revoke would live |
| Telling clients why a sign-up failed | Two more variants in `AccountError` and the Edge's `ApiError`: `signups_closed`, `invalid_signup_code` |

Health checks are a hand-written route next to the generated ones, like
replication and the WebSocket gateway.

## Phase 7 (everyday chat)

No TYML changes. Reactions, typing, and renaming and deleting guilds and
channels needed:

| Need | How |
|---|---|
| Fields added to stored types (who made a channel, a message's reactions) | Optional fields (`created_by: string?`, `reactions: [Reaction]?`), as in phase 2; the public `Message` keeps `reactions` required and the Edge fills in an empty list |
| Reactions as each reader sees them ("did I react?") | Two types for one idea: the cell stores who reacted (`Reaction { emoji, user_ids }`), the public API gives `Reaction { emoji, count, me }`, which the Edge computes per reader |
| Reaction events that can be applied twice | The event carries the emoji's count after the change (`ReactionChange.count`) |
| Typing, which nothing should keep | A log entry like any other (`typing`), not forwarded by the outbox and trimmed with the log |
| New events | More `kind` values with one optional payload field each (`guild_info`, `reaction`, `typing`), as before |
| Room for a guild's own emoji later | `emoji: string`, with `custom:<id>` reserved in its doc comment; no union type needed |

## Phase 7 (attachments)

No TYML changes. Files sent with messages, on a new Media service
(`types/media.tyml`), needed:

| Need | How |
|---|---|
| File contents, which TYML has no type for | Routes written by hand next to the generated ones (`POST /media/upload`, `GET /media/…` on Edges; `PUT`/`GET /media/blobs` on Media services), described in the TYML files' comments, like the WebSocket gateway; what they answer is a TYML type (`Attachment`, `Blob`, `ApiError`/`MediaError` as JSON). Base64 strings in JSON would have worked with no route of our own, at a third more bytes and whole files in memory |
| A file's kind (attachment now, emoji later) deciding who may read it | An enum (`BlobKind`) in the stored record |
| Fields added to stored and public types (a message's files; a message's files to delete) | Optional fields (`attachments: [Attachment]?`, `DeletedMessage.attachments: [string]?`); the public `Message.attachments` is required and the Edge fills in an empty list, as for reactions |
| Where a file is, without a lookup | Its ID (`media-0.3f9c…`), a string, as invite codes carry their guild |
| Pages of files oldest first, for maintenance | `after: string?` cursor and `next: string?`, as in `export_guild` |
