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

## Phase 5 — real translation, choosing models (done)

Translation runs on real models, and guilds choose which:

- Engines serve several models from a models file: local ones
  (OpenAI-compatible servers: llama.cpp, vLLM, Ollama) and hosted ones
  (OpenAI and others through the same protocol; Claude through the native
  Messages API with prompt caching). Hosted ones go through the egress
  proxy.
- Two prompt styles: JSON for instruction-following models, plain text one
  target at a time for translation models (Hy-MT2), with summaries kept
  without the model for those.
- Measured on a laptop (Intel Iris Xe, llama.cpp over Vulkan): Hy-MT2 1.8B
  translates a chat message into two or three languages in about 3 s and
  is the local default (`deploy/local/llm.sh`); Qwen3.5 2B and 4B compared.
  A tool to run the same chat through any models (`sample_chat`).
- Translation Cells use every engine of the cluster: one catalog of
  models, a space's requests always starting at the same replica of its
  model, the default model for spaces that chose none or one that is gone.
- Owners of paid guilds choose the model: the guild's language settings in
  the desktop app, `/model` in the terminal client.
- Plans are set by operators only (`babel-admin set-plan`), never by a
  tenant for itself.

## Phase 6 — running it for real

BabelChat on real servers, open to people.

Done so far:

- Deployment on NixOS (`deploy/nixos`, `nix/module.nix`): one description
  of the cluster, the machines and which service runs on which, for any
  placement from one machine per service to everything on one. Each
  machine gets WireGuard to the others, a firewall opening services on
  WireGuard only, hardened systemd units with their certificates as
  credentials, standbys, llama.cpp for a local model, and Caddy with
  Let's Encrypt in front of its Edges. `deploy.sh` makes the keys and
  certificates, installs NixOS over a provider's Debian (nixos-anywhere),
  and builds, copies and activates, keeping secrets out of the Nix store.
  An eight-machine example (`deploy/vultr`); the module tested on virtual
  machines.
- An open server's basics: sign-ups open, by invitation (codes from
  operators) or closed; password changes; signing out other devices, whose
  tokens every Edge refuses at once (a stream of revocations, like
  placements).
- Health checks (`babel-admin health`), logs as JSON lines.
- Metrics: every service counts and times its calls, plus what matters for
  it (translation times and tokens, sessions, standbys behind, backups);
  on NixOS, Prometheus and Grafana on one machine with a dashboard and
  alert rules (`deploy.sh dashboard`).
- Failing over in one command (`deploy.sh failover <service|machine>`):
  standbys take over, the old primary is stopped or, if lost, cut off
  from WireGuard so it cannot come back as a second primary (otherwise
  it becomes the new standby). Drilled on the eight machines, planned
  and with a machine lost: 6 to 10 seconds unavailable, no acknowledged
  message lost.
- Backups on another machine: one machine fetches every store's backups
  over WireGuard every 15 minutes; `deploy.sh restore <service>
  [--from-copy]` puts a service back as of its newest backup, its standby
  starting over.
- Certificates renewed and the CA and the token key replaced without
  stopping anything (`deploy.sh renew`, `rotate ca`, `rotate token-key`):
  both old and new trusted until everything has the new; an alert 30
  days before a certificate expires.
- Running it: the eight machines on Vultr, installed and updated with
  `deploy.sh`, chatting over HTTPS (the terminal client and the desktop
  app) with translations from Hy-MT2 on a CPU.
- Measured on a rented GPU (NVIDIA A16): what one GPU translates per
  second and per dollar, with a load test (`load_test`, `deploy/bench`).

Next:

- GPU engines in the NixOS module.
- Operations: automatic failover; sending alerts; backups in another
  region (object storage); moving a guild's Translation Cell space; a
  replicated Directory; machines without Nix (plain systemd units).
- Translation from real usage: window and summary sizes tuned with real
  numbers; reference channels (owner-chosen descriptions) as context.
- Translation Cell as a standalone, self-hostable product for other
  platforms; Server SDK documentation.

## Phase 7 — the product

Done so far:

- Everyday chat, in the SDK, the terminal client and the desktop app:
  reactions with any Unicode emoji (a count each, the reader's own
  marked, room left for a guild's own emoji usable in other guilds);
  seeing who is writing; messages of several lines (a multi-line
  `TextField` in `gpui-md3e`); renaming and deleting guilds and channels;
  starting threads from messages, renaming and deleting them, from the
  desktop app too.

Next:

- BabelChat Cloud engine; billing setting plans (and which models each
  plan may use; owners bringing their own API keys); Discord account linking.
- Managed Translation Cells for platforms that do not want to host one.
- Mirrored hosting for the on-device models, and model updates.
- The desktop app: categories from the UI, keyboard navigation, tray
  icon.
- Chat features not built yet: roles beyond owner and member, attachments
  (and with them a guild's own emoji, usable in other guilds too), who
  reacted, push notifications to devices that are offline.
- Before the first release, protocol compatibility. Until then contracts
  change freely, and a client only works against servers built from the
  same tree. Then:
  - generated code skips enum values it does not know (an event kind from
    a newer server) and reads a missing array as empty;
  - contracts only grow: new response fields optional, removals in two
    steps (both, then the new one alone), written down in
    `docs/tyml-changes.md`;
  - clients send their version, and Edges answer a too-old one with
    "update", which the apps show instead of "cannot connect";
  - optionally, apigen warns when a contract breaks the released one.

## Open questions

- Retention of translation events and windows per plan.
- Whether free guilds get an English LLM translation to pivot from (today
  they translate from the original on device).
