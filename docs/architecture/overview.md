# Architecture overview

## Goals

- **Zero-click translation.** Readers see messages in their language without
  asking. Free guilds translate on the reader's device; paid guilds get
  context-aware LLM translation as soon as a message is sent.
- **Translation separate from messaging.** Other platforms can adopt the
  translation half (the Translation Cell) and keep their own message store.
  BabelChat's own chat is just the first platform to use it.
- **Guilds spread over many servers**, without revealing which server holds
  a guild.
- **Self-hosting**, from a local LLM behind the engine to the whole service in
  one process.

## The layers

```
                      internet
  clients ── HTTPS ──▶ Edge × N            the only public servers
  (client SDK)
═════════════════════════│═══════════════════════════════════ internal network
                         ├─▶ Account           login, tokens, settings (closed in production)
                         ├─▶ Directory         guild -> cells, user -> guilds
                         ├─▶ Message Cell × N  guilds, channels, members, original messages
                         └─▶ Translation Cell × N
                                 │  translations, context, language policy
                                 ▼
                             Translation Engine   stateless: mock, local LLM, or cloud
```

| Layer | Holds | Scales by |
|---|---|---|
| [Edge](edge.md) | nothing durable: sessions, caches | adding Edges |
| [Directory](directory.md) | placement table, membership index | small; replicate |
| [Message Cell](message-cell.md) | a set of guilds | adding cells, moving guilds |
| [Translation Cell](translation-cell.md) | translations of a set of spaces | adding cells |
| [Translation Engine](translation-engine.md) | nothing | adding replicas |

On the other side of the Edge, the [client SDK](client.md) keeps a session
live, decides which text each reader sees and translates on device.

Every boundary is an API defined in TYML (see [API definitions](../api.md)).
Services only see each other through the generated traits, so a service can
run in-process (as in `babel-server` today) or behind an HTTP client without the caller
noticing.

## A message, end to end

```
1. Client ─ send_message ─▶ Edge           checks the token, rate-limits,
                                           resolves the guild's cells
2. Edge ─▶ Message Cell                    one atomic write:
                                           message + log entry + outbox entry
3. Message Cell log ─▶ Edges ─▶ clients    "message_created" (original text)
                                           free readers translate on device now
4. Outbox ─ ingest ─▶ Translation Cell     detects the language, appends the
                                           message to the conversation window,
                                           queues a job; all in one write
5. Translation Cell ─▶ Engine              N to N: from the source straight into
                                           every LLM language of the guild
6. Translation Cell log ─▶ Edges           "message_translated", filtered per
                       ─▶ clients          reader to their language + English
```

Steps 3 and 5 overlap: readers see the original (or an on-device
translation) at once and the LLM translation replaces it a moment later.

## A new guild

```
Client ─ create_guild ─▶ Edge ─▶ Directory: pick the least loaded cells,
                                            create the guild on the Message Cell,
                                            commit the placement (version v)
                         Edge ◀─ placement   this Edge knows immediately
                         Edge ─▶ Translation Cell: initial language policy
Directory ─ watch_placements ─▶ other Edges  usually within milliseconds
other Edge, guild unknown ─ get_placement ─▶ Directory   the fallback
```

No step waits for every Edge to acknowledge, so one slow Edge cannot block
guild creation.

## Code map

| Concept | Code | Contract |
|---|---|---|
| Public API, sessions, fan-out | `server/edge` | `types/edge.tyml` |
| Placement, membership index | `server/directory` | `types/directory.tyml` |
| Guild actors, event log, outbox | `server/message-cell` | `types/message_cell.tyml` |
| Translation pipeline, context, policy | `server/translation-cell` | `types/translation_cell.tyml` |
| Prompts, caching, backends | `server/engine` | `types/engine.tyml` |
| Accounts (stub) | `server/account` | `types/account.tyml` |
| Language normalization | `crates/babel-lang` | — |
| Storage | `server/store` | — |
| Wiring | `server/all-in-one` | — |
| Client SDK: session, display, on-device translation | `client/sdk` | `types/edge.tyml` (generated client) |
| Terminal client | `client/cli` | — |
| Desktop app (GPUI + Material 3 Expressive) | `client/desktop` | — |

## What is open and what is closed

| Open source (this repository) | Closed |
|---|---|
| Edge, Directory, Message Cell, Translation Cell | Account service (billing, Discord linking) |
| Engine API and the reference engine | BabelChat Cloud engine (tuned prompts, model routing) |
| `babel-lang`, the API definitions | |

The closed parts implement the same APIs as their open counterparts, so
everything here runs without them.
