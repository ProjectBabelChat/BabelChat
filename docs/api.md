# API definitions

Every boundary between services is defined in [TYML](https://tyml-org.github.io/tyml-lang.org/)
under `types/`, one file per boundary. Rust code is generated from them into
`server/api/` and committed, so building BabelChat needs only cargo.

| File | Boundary | Server crate | Client crate |
|---|---|---|---|
| `edge.tyml` | clients → Edge (public) | `babel-api-edge` | `babel-api-edge-client` |
| `account.tyml` | Edge → Account | `babel-api-account` | |
| `directory.tyml` | Edge, Message Cells → Directory | `babel-api-directory` | |
| `message_cell.tyml` | Edge, Directory → Message Cell | `babel-api-message-cell` | |
| `translation_cell.tyml` | platforms → Translation Cell | `babel-api-translation-cell` | `babel-api-translation-cell-client` |
| `engine.tyml` | Translation Cell → Engine | `babel-api-engine` | `babel-api-engine-client` |

A server crate contains the types, one async trait per `interface`, and an
Axum router (`router(api)`, `serve(api, address)`). A service implements the
traits of its file; callers hold `Arc<dyn Trait>`. Client crates contain the
same types and an HTTP client per interface; `babel_common::convert` moves a
value between a server crate's type and a client crate's type, whose JSON is
identical by construction.

## Regenerating

```sh
cargo run --manifest-path tools/apigen/Cargo.toml
```

`tools/apigen` depends on a local TYML checkout next to this repository
(`../tyml`) while generator changes are in flight, so it is kept out of the
main workspace. Do not edit `server/api/*` by hand.

## Conventions

Things TYML does not have a dedicated feature for, expressed with existing
ones:

- **One-of payloads.** Events carry a `kind` enum and one optional field per
  payload; exactly the field named by `kind` is set (`GuildEvent`,
  `TranslationEvent`, `Event`).
- **Streams.** Long polls: `poll_events(after, wait_ms) -> { events, next, head }`
  returns as soon as there are events after `after`, or after `wait_ms`
  without any. `head` lets a follower start at the end. The same shape works
  in-process and over HTTP. Clients can also take the gateway's batches
  over a WebSocket (`/gateway/ws`), which carries the same `EventBatch`
  type and is documented on the `Gateway` interface.
- **Maps.** Arrays of pairs (`[LanguageAlias]`) where order matters or keys
  are user input.
- **Timestamps.** `int` milliseconds since the Unix epoch.
- **IDs.** `string`, so JavaScript clients never lose precision. Message
  IDs and read markers are time-ordered integers: compare them as numbers.
- **Evolving stored types.** Fields added to types that services store are
  optional, so older records still decode.
- **Errors.** One enum per file, used with `throws`. The generated router
  answers 400 with the error as JSON; 401 for a missing or invalid token.

## HTTP shape

Generated routes are `/{interface}/{function}` in snake case: `POST
/guilds/create_guild`, `GET /guilds/get_guild`. Bodies are JSON; other
arguments are JSON values in the query string (`?guild_id="abc"`). See
[development](development.md) for curl examples.
