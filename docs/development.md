# Development

## Running BabelChat

```sh
nix develop          # provides libclang for RocksDB's bindings
cargo run -p babel-server -- --data ./data --plan paid3
```

| Option | Default | |
|---|---|---|
| `--listen` | `127.0.0.1:7000` | public Edge API |
| `--data` | `data` | one subdirectory per service |
| `--message-cells`, `--translation-cells` | `2`, `2` | cells to spread guilds over |
| `--plan` | `free` | plan of new guilds (`free`, `paid1`, `paid2`, `paid3`) |
| `--daily-budget` | none | LLM-translated messages per guild per day |
| `--engine-url` | in-process mock | an Engine API server, e.g. `babel-engine` |
| `--mock-latency-ms` | `300` | simulated model latency of the mock |

Without Nix, install clang (libclang) and a C++ compiler for RocksDB.

## Chatting in the desktop app

`babelchat` (`client/desktop`) is the desktop app. Each user gets a
profile; run one window per user. A Japanese and an English speaker on one
machine, with real on-device translation (free plan):

```sh
cargo run -p babel-server -- --plan free                 # terminal 0

cargo run -p babel-desktop -- --profile alice            # terminal 1
cargo run -p babel-desktop -- --profile bob              # terminal 2
```

1. In each window, "Create account": a name, a password and the language
   you read (`ja-JP` for alice, `en-US` for bob; it defaults to the
   system's). The window's own words follow that language.
2. Alice: **+** in the left rail → "Create a guild", a name and its
   languages (hers and English are preselected). The guild starts with
   `#general`.
3. Alice: the guild's name (or the person icon in the channel header) →
   "Invite people" → "Copy".
4. Bob: **+** → "Join a guild", paste the code, Enter.
5. Write. Alice writes Japanese and reads Bob in Japanese; Bob reads her
   in English. The first message of each direction downloads its model
   (35–55 MB) into `~/.cache/babelchat/models`.

Hover a message to react, reply, start a thread, edit or delete it;
Shift+Enter starts a new line; `@bob` mentions Bob. Other
channels show unread dots and mention counts; while the window is in the
background, new messages become desktop notifications.

| Option | Environment | Default |
|---|---|---|
| `--profile` | `BABEL_PROFILE` | `default` |
| `--server` | `BABEL_SERVER` | `http://127.0.0.1:7000` (prefilled on the sign-in screen) |
| `--config-dir` | `BABEL_CONFIG_DIR` | `~/.config/babelchat` (shared with `babel`) |
| `--device`, `--models-dir`, `--transport`, `--no-cache` | as for `babel` | |
| `--theme` | `BABEL_THEME` | `system` (`light`, `dark`) |
| `--size` | | `1280x800` |
| `--no-system-notifications` | | desktop notifications on |

Profiles are shared with the terminal client: `babel --profile alice`
works with a sign-in made in the window, and the other way round.

`BABEL_SCRIPT` makes the window drive itself (clicks, keys, text,
screenshots with `grim`), which is how the app is checked without a person
at the keyboard; see [the desktop app](architecture/desktop.md#checking-it-without-a-person).

## A cluster on one machine

The same, with every service in its own process, talking over mutual TLS as
they would across machines (see [deployment](deployment.md)):

```sh
nix develop -c deploy/local/run.sh --plan free             # terminal 0

cargo run -p babel-desktop -- --profile alice --server http://127.0.0.1:7000   # edge-0
cargo run -p babel-desktop -- --profile bob --server http://127.0.0.1:7001     # edge-1
```

Alice and Bob sit on different Edges and chat as above. Logs are in
`deploy/local/logs`, data and backups in `deploy/local/data`. While they
chat, an operator can move their guild to the other Message Cell; writes
pause for a moment and nothing else shows:

```sh
bin=target/debug
admin="/babel-admin --cluster deploy/local/cluster.toml --name ops"
 placements                          # guild id, message cell, translation cell
 move-guild <guild id> message-cell-1
```

A standby of a cell, and a restore from its backups:

```sh
/babel-standby --cluster deploy/local/cluster.toml --name message-cell-0 \
  --data deploy/local/data/message-cell-0-standby
/babel-admin restore --backup-dir deploy/local/data/backups/message-cell-0 \
  --data /tmp/restored-message-cell-0
```

Profiles made against `babel-server` hold tokens it signed; sign in again
against the cluster (or use other profile names).

## Chatting from a terminal

`babel` (`client/cli`) is a line-mode chat client on the client SDK. Each
user gets a profile; open one terminal per user.

```sh
cargo run -p babel-server -- --plan paid3          # terminal 0

alias babel='cargo run -q -p babel-cli --'
babel --profile alice register alice --locale ja-JP   # prompts for a password
babel --profile alice                                 # terminal 1: chat as alice
/create Rhythm Club
/languages ja en ko llm ja en
/channel general
/invite
Invite code: 4cdcaa222ba2b677de7ab978-a00f4ec5b93ab3a4278d7ff2

babel --profile bob register bob --locale en-GB
babel --profile bob                                   # terminal 2: chat as bob
/join 4cdcaa222ba2b677de7ab978-a00f4ec5b93ab3a4278d7ff2
```

Then type. Alice writes Japanese; Bob reads English:

```
#general #1 alice: [en-US] 明日の夜、一緒に遊びませんか  ‹ja-JP→en-US›
```

The mock engine marks LLM translations with the target tag
(`[en-US] …`). On-device translations are real: the Firefox Translations
(Bergamot) models, downloaded on first use of each direction (35–55 MB)
into `~/.cache/babelchat/models`. A reader outside the guild's languages,
or any reader of a `--plan free` server, sees them:

```
#general #3 carol: いいアイデアですね、何時ですか?  ‹fr-FR→en→ja-JP on device›
```

`--device mock` swaps in a mock that marks device translations instead
(`[device fr-FR] [en-US] …` is French on device, from the LLM's English). `/help` lists the commands: channels,
threads (`/thread #1 name`), `/reply #1 …`, `/edit #1 …`, `/delete #1`,
`/react #1 👍` (or `:thumbsup:`), renaming and deleting channels and
guilds, `/locale fr-FR`, `/original on` and more. A line ending in `\`
goes on on the next one, for messages of several lines.

The open channel prints its messages; other channels speak up through
notifications, already translated:

```
🔔 mention #random #2 alice: [en-US] @bob 見て  ‹ja-JP→en-US›
```

`@name` mentions someone, `/reply #n` answers a message (and notifies its
author), `/unread` lists unread channels with their mention counts, and
`/notify mentions` (or `all`, `none`; add `here` for the open channel only)
chooses what notifies you. Owners `/kick` and `/ban` members; `/leave`,
`/invites`, `/bans`, `/unban` and `/status` (WebSocket, long polling or
offline) do what they say. Each profile keeps a cache next to its sign-in,
so the next start shows the last messages at once, offline too.

`babel password` changes the password and `babel signout-others` signs the
profile's other devices out (both keep this one signed in). On a server
that takes accounts by invitation, `register` needs `--signup-code`
(`BABEL_SIGNUP_CODE`).

| Option | Environment | Default |
|---|---|---|
| `--server` | `BABEL_SERVER` | `http://127.0.0.1:7000` (used by `register` and `login`) |
| `--profile` | `BABEL_PROFILE` | `default` |
| `--config-dir` | `BABEL_CONFIG_DIR` | `~/.config/babelchat` (sign-ins and caches, mode 0600) |
| `--transport` | `BABEL_TRANSPORT` | `auto` (WebSocket, falling back to long polling), `websocket`, `poll` |
| `--no-cache` | | start without the cache and write none |
| `--device` | `BABEL_DEVICE` | `bergamot` (real models) or `mock` |
| `--models-dir` | `BABEL_MODELS_DIR` | `~/.cache/babelchat/models` |
| password | `BABEL_PASSWORD` | prompted |
| new password (`babel password`) | `BABEL_NEW_PASSWORD` | prompted |

Without a terminal, input can be piped; the client exits at end of input:

```sh
printf '/join %s\nhello!\n' "$CODE" | babel --profile bob
```

## Trying it with curl

```sh
U=http://127.0.0.1:7000
json='content-type: application/json'

# Register two users; keep their tokens.
curl -s $U/auth/register -H "$json" -d '{"name":"alice","password":"correct horse","locale":"ja-JP"}'
curl -s $U/auth/register -H "$json" -d '{"name":"bob","password":"correct horse","locale":"en-GB"}'
A=<alice's jwt>; B=<bob's jwt>

# Alice creates a guild, declares Japanese and English, adds a channel.
curl -s $U/guilds/create_guild -H "authorization: Bearer $A" -H "$json" \
  -d '{"name":"Rhythm Club","description":null}'
G=<guild id>
curl -s $U/guilds/put_language_policy -H "authorization: Bearer $A" -H "$json" \
  -d "{\"guild_id\":\"$G\",\"languages\":[\"ja\",\"en\"],\"llm_languages\":[\"ja\",\"en\"],
       \"region_mode\":\"merged\",\"aliases\":[],\"translate_regional_variants\":false}"
curl -s $U/guilds/create_channel -H "authorization: Bearer $A" -H "$json" \
  -d "{\"guild_id\":\"$G\",\"kind\":\"text\",\"name\":\"general\",\"topic\":null,
       \"parent_id\":null,\"starter_message_id\":null}"
C=<channel id>

# Alice invites Bob; Bob joins with the code.
curl -s $U/guilds/create_invite -H "authorization: Bearer $A" -H "$json" \
  -d "{\"guild_id\":\"$G\",\"max_age_secs\":null,\"max_uses\":null}"
I=<invite code>
curl -s -X POST -G $U/guilds/join_guild -H "authorization: Bearer $B" --data-urlencode "invite=\"$I\""

# Alice writes in Japanese; Bob reads English.
curl -s $U/messages/send_message -H "authorization: Bearer $A" -H "$json" \
  -d "{\"guild_id\":\"$G\",\"channel_id\":\"$C\",\"text\":\"明日の夜に駅の近くで集まりませんか\"}"
curl -s -G $U/messages/get_messages -H "authorization: Bearer $B" \
  --data-urlencode "guild_id=\"$G\"" --data-urlencode "channel_id=\"$C\"" \
  --data-urlencode 'before=null' --data-urlencode 'limit=10'
# ... "translations":[{"lang":"en-US","text":"[en-US] 明日の夜に..."}]   (mock engine)
```

For live events, `POST /gateway/connect` returns a session, and
`GET /gateway/poll` long-polls it (`session_id="..."`, `after=0`,
`wait_ms=25000`, each JSON-encoded like the other query arguments). The
same events stream over a WebSocket, with plain query arguments:

```sh
websocat -H "authorization: Bearer $B" "ws://127.0.0.1:7000/gateway/ws?session_id=$S&after=0"
```

## Translating with a local model

Any OpenAI-compatible server works: llama.cpp's `llama-server`, vLLM, Ollama,
LM Studio. `deploy/local/llm.sh` serves Hy-MT2 1.8B with llama.cpp (from
nixpkgs, over Vulkan, so integrated GPUs help; `LLM_DEVICE=cpu` for the
CPU), downloading the model on first run; `deploy/local/models.toml`
describes it to the engine. About 3 s per message on an Intel Iris Xe (see
[Translation Engine](architecture/translation-engine.md#measured-on-a-laptop)).

```sh
# 1. the model server (127.0.0.1:8001)
deploy/local/llm.sh

# 2a. the local cluster translating with it
BABEL_MODELS=models.toml nix develop -c deploy/local/run.sh --plan paid3

# 2b. or babel-server, with the reference engine in front of the model
cargo run -p babel-engine -- --models deploy/local/models.toml --listen 127.0.0.1:7100
cargo run -p babel-server -- --plan paid3 --engine-url http://127.0.0.1:7100
```

The models file can list several models, local and hosted (OpenAI,
Anthropic); owners of paid guilds choose among them in the guild's language
settings (desktop app) or with `/model` (terminal client). Guilds created
before switching plans keep theirs; operators change one with
`babel-admin set-plan <guild> <plan>` (cluster).

To compare models before choosing one:

```sh
cargo run -p babel-engine --example sample_chat -- --models deploy/local/models.toml
```

A laptop has 16 GB or so shared by the model, the cluster and the build:
stop model servers before building the desktop app or the whole workspace.

## Measuring a model server

`load_test` runs the same chat in N guilds at once (each with its own
context, its window growing) and prints, per N, how long a message waits
for its translations and how many the server finishes per second: how
many guilds one GPU carries, and what a translation costs.

```sh
cargo run --release -p babel-engine --example load_test -- \
  --models deploy/bench/models.toml --model hy-mt2-7b --guilds 1,2,4,8,16
```

For a rented GPU machine (Ubuntu with NVIDIA's driver),
`deploy/bench/gpu.sh` builds llama.cpp for CUDA, fetches the models and
serves one at a time with N parallel slots; the load test reaches it
through a tunnel:

```sh
scp deploy/bench/gpu.sh root@<gpu>: && ssh root@<gpu> ./gpu.sh setup
ssh root@<gpu> ./gpu.sh serve hy-mt2-7b 8
ssh -N -L 8001:127.0.0.1:8001 root@<gpu> &     # then load_test as above
```

## Tests

```sh
cargo test -p babel-lang -p babel-store -p babel-common -p babel-engine \
  -p babel-translation-cell -p babel-message-cell -p babel-directory \
  -p babel-account -p babel-edge -p babel-server -p babel-sdk -p babel-cli \
  -p babel-bergamot -p babel-desktop -p gpui-md3e -p babel-net
cargo test -p babel-bergamot -- --ignored   # downloads three models
nix build .#checks.x86_64-linux.cluster      # the NixOS module, on two VMs
```

| Crate | What the tests show |
|---|---|
| `babel-lang` | normalization rules, viewer resolution |
| `babel-store` | key ordering, scans, RocksDB and in-memory stores behave alike; standbys replaying a primary from a checkpoint; backups restoring |
| `babel-net` | cluster files; certificates naming services; mutual TLS that tells servers who calls and refuses roles and misdirected clients; a standby following a live service; the egress proxy refusing internal addresses and plain HTTP; who may call what (only operators set plans and make sign-up codes) |
| `babel-account` | registration and sign-in; names are one mentionable word; tokens verify with the public key only; sign-ups by invitation or closed; signing out other devices and changing the password revoke older tokens, in a stream that survives restarts |
| `babel-directory` | the membership stream, and its trimming |
| `babel-engine` | deterministic, injection-safe prompts; growing windows keep hitting the simulated cache; plain prompts sharing their background across targets; models files; requests choosing a model; OpenAI-compatible and Anthropic request shapes (thinking turned off, cache markers); remote engine over HTTP |
| `babel-translation-cell` | N-to-N translation, free spaces, edits, thread context, window folding, budgets, jobs resumed after a restart; tenants kept apart by certificate; log trimming; spaces choosing a model; one catalog from several engines; a space sticking to one replica until it fails |
| `babel-message-cell` | outbox delivery, roles, invites, history paging, long polls; mentions, replies and read markers; leaving, kicks and bans; notification settings; log trimming that spares undelivered entries; reactions counted once per member; typing through the log only; renaming; deleted channels taking their threads and messages (and their Translation Cell conversations), deleted categories freeing their channels; deleted guilds read to the end of their log, then gone from the Directory and the Translation Cell |
| `babel-edge` | sessions that fall behind their queue must reconnect; removals spare sessions that joined again |
| `babel-server` | the whole system over HTTP with the generated client; probing resistance; crash recovery of the real binary; SDK sessions reconnecting across a server restart; split up over mutual TLS: people on different Edges chatting, a user's devices on two Edges staying in step, guilds moving between cells while people write |
| `babel-sdk` | saved sign-ins; display decisions, device chains through English; paid and free guilds, edits, deletions, history paging, locale changes, invites against an in-process server; notifications in the reader's language, unread markers synced across devices, leaving and bans, starting from the cache offline, WebSocket and long polling; signing the other devices out (they are signed out, this one carries on live) and changing the password; reactions with each reader's own marked; others writing, until their message (of several lines) arrives; threads, channels and guilds renamed and deleted for everyone |
| `babel-bergamot` | tag-to-model mapping, byte pieces; with `--ignored`, real models downloaded and run (ja→en, en→ja, en→fr) |
| `babel-cli` | two people chatting through the terminal client, as typed; notifications from other channels, replies, kicks; lines continued with `\`, reactions by emoji or shortcode, renaming and deleting with the name typed to confirm |
| `babel-desktop` | channel order, avatars, the UI's words in both languages; the screens themselves are checked by scripted runs (`BABEL_SCRIPT`) |

## Changing an API

1. Edit `types/*.tyml`.
2. `cargo run --manifest-path tools/apigen/Cargo.toml`.
3. Fix the implementations the compiler points at.
