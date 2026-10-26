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
`/locale fr-FR`, `/original on` and more.

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
LM Studio.

```sh
# 1. a model server, e.g.
llama-server -m hy-mt2-1.8b-q8_0.gguf --port 8000

# 2. the reference engine in front of it
cargo run -p babel-engine -- --backend openai \
  --base-url http://127.0.0.1:8000/v1 --model hy-mt2 --listen 127.0.0.1:7100

# 3. BabelChat using it
cargo run -p babel-server -- --plan paid3 --engine-url http://127.0.0.1:7100
```

`--api-key-env NAME` passes a key from an environment variable;
`--no-json-mode` turns off `response_format` for servers without it. The
backend is tested against a fake server; it has not been run against a real
model yet.

## Tests

```sh
cargo test -p babel-lang -p babel-store -p babel-common -p babel-engine \
  -p babel-translation-cell -p babel-message-cell -p babel-directory \
  -p babel-account -p babel-edge -p babel-server -p babel-sdk -p babel-cli \
  -p babel-bergamot
cargo test -p babel-bergamot -- --ignored   # downloads three models
```

| Crate | What the tests show |
|---|---|
| `babel-lang` | normalization rules, viewer resolution |
| `babel-store` | key ordering, scans, RocksDB and in-memory stores behave alike |
| `babel-account` | registration and sign-in; names are one mentionable word |
| `babel-engine` | deterministic, injection-safe prompts; growing windows keep hitting the simulated cache; OpenAI-compatible request shape; remote engine over HTTP |
| `babel-translation-cell` | N-to-N translation, free spaces, edits, thread context, window folding, budgets, jobs resumed after a restart |
| `babel-message-cell` | outbox delivery, roles, invites, history paging, long polls; mentions, replies and read markers; leaving, kicks and bans; notification settings |
| `babel-edge` | sessions that fall behind their queue must reconnect; removals spare sessions that joined again |
| `babel-server` | the whole system over HTTP with the generated client; probing resistance; crash recovery of the real binary; SDK sessions reconnecting across a server restart |
| `babel-sdk` | display decisions, device chains through English; paid and free guilds, edits, deletions, history paging, locale changes, invites against an in-process server; notifications in the reader's language, unread markers synced across devices, leaving and bans, starting from the cache offline, WebSocket and long polling |
| `babel-bergamot` | tag-to-model mapping, byte pieces; with `--ignored`, real models downloaded and run (ja→en, en→ja, en→fr) |
| `babel-cli` | two people chatting through the terminal client, as typed; notifications from other channels, replies, kicks |

## Changing an API

1. Edit `types/*.tyml`.
2. `cargo run --manifest-path tools/apigen/Cargo.toml`.
3. Fix the implementations the compiler points at.
