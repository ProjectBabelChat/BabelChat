# Security

## Unreachable rather than hidden

Only Edges have public addresses. Cells, the Directory and engines live on the
internal network and accept connections only from inside (WireGuard plus
mutual TLS in split deployments). Knowing which cell holds a guild therefore
does not let anyone attack it directly.

Inside, every service holds a certificate from the cluster's own CA that
names it (`babel://message-cell/message-cell-0`). Servers refuse callers
without one and check each call against a fixed table of who may call what
(`server/net/src/policies.rs`): an Edge cannot copy a cell's store or move
guilds, a tenant cannot reach a Message Cell. Clients check that the server
holds the certificate of the service they meant to reach. See
[deployment](../deployment.md).

Metrics are not behind mutual TLS: services and `node_exporter` answer
`GET /metrics` in plain HTTP, on addresses the firewall opens to WireGuard
only. They carry counts and timings by route, model and service, never a
user, guild or message; Grafana and Prometheus listen on 127.0.0.1 of the
monitoring machine, reached through SSH.

Keeping placement secret still matters against subtler attacks — for
instance, making a cell slow by abusing another guild on it, to degrade a
target guild. So placement does not leak either:

| Leak | Countermeasure |
|---|---|
| Node numbers inside IDs (Snowflake IDs carry worker numbers) | Guild, channel and session IDs are random; message IDs are time + random |
| Cell names in responses or headers | Public types are generated separately and converted field by field (`server/edge/src/convert.rs`) |
| "not found" vs "forbidden" | Both are `not_found` |
| Joining a guild by its ID | Joining needs an invite: a random 96-bit secret checked by the guild's cell, with expiry (7 days by default) and use limits; bad, expired and used-up invites are all `not_found`; so are invites used by a banned user |
| Outgoing requests (link previews, webhooks) revealing a cell's address | Outbound traffic leaves through the egress proxy (`babel-egress`), which also refuses internal addresses |
| Attachment URLs | Served through object storage and a CDN with no cell in the path (planned) |
| Correlated outages | Cannot be prevented; moving guilds between cells blurs them |

## At the Edge

- Tokens are verified locally; unauthenticated requests never reach a cell.
- Writes are rate-limited per user (token bucket).
- A member who leaves or is removed stops receiving the guild's events as
  soon as the guild's log reaches the Edge, and every request is checked by
  the cell anyway. A session subscribed from a stale membership (the
  Directory's copy lags) is unsubscribed when the cell refuses its snapshot.
- WebSocket upgrades are authorized like requests (bearer token) and only
  for the caller's own session.
- Unknown guild IDs are looked up once and the miss remembered, so random IDs
  cannot flood the Directory.
- L3/L4 floods are handled in front of the Edges (a provider's DDoS
  protection, Cloudflare Spectrum or anycast); Edges are interchangeable, so
  clients reconnect to another one.

## Translation

| Threat | Countermeasure |
|---|---|
| Spam in many languages to run up the LLM bill | Daily budget per space; on-device fallback past it |
| Prompt injection in messages or descriptions | Content embedded as JSON data, never as instructions (`json` models); engines have no tools; outputs missing targets or far longer than the source are rejected |
| A message talking a translation model into a different translation (`plain` models such as Hy-MT2 read text, not JSON) | The prompt tells the model to translate requests as written (Hy-MT2 1.8B does; the 7B model once answered an injection instead, [measured](translation-engine.md#measured-on-a-rented-gpu)); the harm stays within the author's own message, whose original every reader can show; spaces that need more pick a `json` model |
| A tenant giving itself a better plan | `set_plan` only takes operators' calls (billing later) |
| A malicious owner's description | Only affects that owner's space: context layers are per space |
| Private channel content leaking through context | Only channels everyone in the guild can read may become reference context (planned with reference channels) |
| A self-hosted engine URL used for SSRF | Engine URLs are operator configuration; so are models and their API keys (environment variables of the engine); hosted models are reached through the egress proxy, which only tunnels to public addresses (checking the address it connects to, so DNS rebinding gains nothing) |
| Another platform reading BabelChat's translations, or another platform's | The tenant comes from the caller's certificate; every key starts with it |

## Accounts

Passwords are hashed with Argon2. Tokens are signed with Ed25519 (EdDSA) by
the Account service, which alone holds the private key; Edges verify with
the public key, so a compromised Edge cannot mint tokens.

| Threat | Countermeasure |
|---|---|
| A stolen token, or a device left signed in | The user signs the other devices out (or changes the password): every token issued until then is revoked. Edges follow the Account service's stream of revocations and refuse them at once, still without calling it per request, and end their sessions. A revocation is kept a token lifetime (seven days), after which the tokens it covers have expired anyway |
| Guessing the current password through `change_password` | It is rate-limited like other writes, per user |
| Strangers filling an open server | Sign-ups by invitation: codes from operators, with uses and expiry (`--signups invite`), or none at all |
| An Edge started while the Account service is down | It accepts revoked tokens until it has read the revocations once (it retries every second) |

## Data handled by the translation side

The engine stores nothing. Translation Cells keep original text only in
conversation windows (the last few dozen messages) and in queued jobs.
Platforms that adopt the Translation Cell can host it themselves, keeping
their users' text on their own servers.
