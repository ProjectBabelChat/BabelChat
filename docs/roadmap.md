# Roadmap

## Phase 0 — the shape of the system (done)

Every service exists with its real boundaries, in one process:

- API contracts for every boundary, generated with TYML.
- Edge: tokens, rate limits, placement cache (insert / watch / pull on miss),
  joined answers, per-reader language views, long-poll gateway with per-guild
  fan-out.
- Directory: placement of new guilds on the least loaded cells, versioned
  placement stream, membership index.
- Message Cell: guilds, channels, threads, roles, messages with time-ordered
  IDs, event log, outbox.
- Translation Cell: language policies with plans, normalization and aliases;
  source detection; immediate N-to-N translation; context hierarchy (space,
  nodes, thread forks, summaries, windows); durable jobs; budgets; usage.
- Engine: one API with mock, OpenAI-compatible and remote implementations;
  layered, cache-friendly prompts; a simulated prompt cache.
- RocksDB storage, one data directory per service; crash recovery.

## Phase 1 — chat without a UI (done)

People can chat, translated, from a terminal:

- Client SDK (`client/sdk`): sign-in, a live session that reconnects and
  reloads by itself, local state, a display decision per message (original,
  LLM, on device, untranslated), on-device translation through English
  behind a `DeviceTranslator` trait (mocked for now).
- Terminal client (`client/cli`, `babel`): saved sign-ins per profile,
  guilds, channels, threads, invites, language policy, edits and deletions,
  settings.
- Invites: joining a guild needs an invite code, checked by the guild's
  cell, with expiry and use limits.
- Gateway: sessions that fell behind their queue are dropped and answer
  `not_found`, so clients know to reload instead of silently missing events.
- Tokens carry the user's name.

## Phase 2 — chat basics (done)

Everything a chat needs apart from the UI, in the SDK and the terminal
client:

- Members leave; owners kick and ban (and unban). Invites can be listed.
- Replies and `@name` mentions, resolved by the server.
- Read markers per member and channel, kept by the server and synced
  across a user's devices; unread channels and mention counts.
- Notification settings per guild and channel (all, mentions, none); the
  SDK raises notifications with the text already in the reader's language,
  and leaves the channel on screen alone.
- WebSocket gateway, with long polling as the fallback.
- A client cache: apps start from what they showed last time, offline too,
  and keep device translations between runs.
- Real on-device translation: the Firefox Translations (Bergamot) models,
  run by fxtranslate behind `DeviceTranslator`, downloaded on first use.

## Phase 3 — the desktop app (done)

A Discord-like desktop app on the SDK (`client/desktop`, `babelchat`),
with GPUI and the Material 3 Expressive components of `gpui-md3e`:

- Sign up and sign in; profiles shared with the terminal client (saved
  sign-ins moved into the SDK).
- Guild rail, channel list, messages, members. Create a guild with its
  languages, invite, join with a code, create channels, leave; owners kick
  and ban.
- Messages in the reader's language with where the translation came from
  and the original on request; replies, mentions, edits, deletions, older
  history on scroll.
- Unread dots and mention counts; notification settings; in-app and
  desktop notifications.
- The UI in English and Japanese, following the language the user reads.
- Scripted runs with screenshots (`BABEL_SCRIPT`), from a driver moved into
  `gpui-md3e`. `TextField` gained masking (passwords) and scrolling to the
  caret.
- Text: the IME placed at the caret; message text selectable, copied and
  dragged; text dragged within and between text fields, and to and from
  other applications (Wayland).

## Phase 4 — split it up (done)

BabelChat runs as it would in production: each service its own process,
on as many machines as needed (`docs/deployment.md`).

- Each service its own binary (`babel-edge`, `babel-account`,
  `babel-directory`, `babel-message-cell`, `babel-translation-cell`,
  `babel-engine`), reaching the others through HTTP clients behind the same
  traits (`babel-net`). One cluster file for all of them; a script that
  runs a whole cluster on one machine (`deploy/local`).
- Mutual TLS between services, with certificates from the cluster's own CA
  that name each service (`babel-pki`); one table of who may call what.
  WireGuard for the network, documented.
- User tokens signed with Ed25519: Edges verify, only the Account service
  signs.
- The Translation Cell takes the tenant from the caller's certificate.
- An egress proxy for outbound traffic that refuses internal addresses
  (`babel-egress`).
- Backups (`BackupEngine`), standbys that replay a service's write-ahead
  log (`babel-standby`), restores (`babel-admin restore`).
- Moving guilds between Message Cells while people write
  (`babel-admin move-guild`); Edges follow a moved guild by themselves.
- Read markers, notification settings and joined guilds reach a user's
  sessions on every Edge.
- Logs trimmed after an hour.

## Phase 5 — real translation

- Moving a guild's Translation Cell space; automatic failover and fencing;
  a replicated Directory; certificate rotation.

- Validate the OpenAI-compatible backend against llama.cpp / vLLM with
  Hy-MT2 and Qwen3; tune window and summary sizes with real usage numbers.
- Space-affinity routing for engine replicas.
- Reference channels (owner-chosen descriptions) as context.
- Translation Cell as a standalone, self-hostable product for other
  platforms; Server SDK documentation.

## Phase 6 — the product

- BabelChat Cloud engine; billing setting plans; Discord account linking.
- Managed Translation Cells for platforms that do not want to host one.
- Mirrored hosting for the on-device models, and model updates.
- The desktop app: multi-line messages, editing guilds and channels,
  threads and categories from the UI, keyboard navigation, tray icon.
- Chat features not built yet: roles beyond owner and member, attachments,
  reactions, typing indicators, renaming and deleting guilds and channels,
  push notifications to devices that are offline.

## Open questions

- Retention of translation events and windows per plan.
- Whether free guilds get an English LLM translation to pivot from (today
  they translate from the original on device).
