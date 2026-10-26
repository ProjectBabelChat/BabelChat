# Security

## Unreachable rather than hidden

Only Edges have public addresses. Cells, the Directory and engines live on the
internal network and accept connections only from inside (WireGuard plus
mutual TLS in split deployments). Knowing which cell holds a guild therefore
does not let anyone attack it directly.

Keeping placement secret still matters against subtler attacks — for
instance, making a cell slow by abusing another guild on it, to degrade a
target guild. So placement does not leak either:

| Leak | Countermeasure |
|---|---|
| Node numbers inside IDs (Snowflake IDs carry worker numbers) | Guild, channel and session IDs are random; message IDs are time + random |
| Cell names in responses or headers | Public types are generated separately and converted field by field (`server/edge/src/convert.rs`) |
| "not found" vs "forbidden" | Both are `not_found` |
| Outgoing requests (link previews, webhooks) revealing a cell's address | All outbound traffic leaves through an egress proxy (planned) |
| Attachment URLs | Served through object storage and a CDN with no cell in the path (planned) |
| Correlated outages | Cannot be prevented; moving guilds between cells blurs them |

## At the Edge

- Tokens are verified locally; unauthenticated requests never reach a cell.
- Writes are rate-limited per user (token bucket).
- Unknown guild IDs are looked up once and the miss remembered, so random IDs
  cannot flood the Directory.
- L3/L4 floods are handled in front of the Edges (a provider's DDoS
  protection, Cloudflare Spectrum or anycast); Edges are interchangeable, so
  clients reconnect to another one.

## Translation

| Threat | Countermeasure |
|---|---|
| Spam in many languages to run up the LLM bill | Daily budget per space; on-device fallback past it |
| Prompt injection in messages or descriptions | Content embedded as JSON data, never as instructions; engines have no tools; outputs missing targets or far longer than the source are rejected |
| A malicious owner's description | Only affects that owner's space: context layers are per space |
| Private channel content leaking through context | Only channels everyone in the guild can read may become reference context (planned with reference channels) |
| A self-hosted engine URL used for SSRF | Engine URLs are operator configuration in Phase 0; per-space custom engines will go through the egress proxy |

## Accounts

Passwords are hashed with Argon2. Phase 0 signs tokens with HS256 and a
per-installation secret; production signs with a private key held only by the
Account service, and Edges verify with the public key, so a compromised Edge
cannot mint tokens.

## Data handled by the translation side

The engine stores nothing. Translation Cells keep original text only in
conversation windows (the last few dozen messages) and in queued jobs.
Platforms that adopt the Translation Cell can host it themselves, keeping
their users' text on their own servers.
