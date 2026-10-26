# BabelChat documentation

BabelChat is a chat service without language barriers: everyone writes and
reads in their own language. These documents describe the servers and the
client SDK as built so far, and where they are heading.

## Reading order

1. [Architecture overview](architecture/overview.md): the four layers, how a
   message travels, and which crate implements what.
2. The components, in request order:
   [Edge](architecture/edge.md),
   [Directory](architecture/directory.md),
   [Message Cell](architecture/message-cell.md),
   [Translation Cell](architecture/translation-cell.md),
   [Translation Engine](architecture/translation-engine.md),
   and on the other side of the Edge the [client SDK](architecture/client.md).
3. Cross-cutting topics:
   [languages](architecture/languages.md),
   [storage](architecture/storage.md),
   [security](architecture/security.md).
4. [API definitions](api.md): the TYML files every boundary is generated from.
5. [Development](development.md): running the server, chatting from a
   terminal, a local LLM, the tests.
6. [Roadmap](roadmap.md): what is done and what comes next.

[TYML changes](tyml-changes.md) lists the changes made to the TYML generator
for BabelChat.

## Where the code is

| Path | What |
|---|---|
| `types/*.tyml` | API contracts, one file per boundary |
| `server/api/*` | Crates generated from them (do not edit) |
| `server/edge` | Edge: the public entry point |
| `server/directory` | Directory: placement and membership index |
| `server/message-cell` | Message Cell: guilds and original messages |
| `server/translation-cell` | Translation Cell: translations, context, language policy |
| `server/engine` | Reference translation engine (mock, OpenAI-compatible) |
| `server/account` | Development stand-in for the closed Account service |
| `server/store` | RocksDB storage shared by the services |
| `server/common` | Small shared helpers |
| `server/all-in-one` | `babel-server`: every service in one process |
| `client/sdk` | Client SDK: sessions, display language, on-device translation |
| `client/cli` | `babel`: a terminal chat client on the SDK |
| `crates/babel-lang` | Language tag normalization, shared with clients |
| `tools/apigen` | Regenerates `server/api` from `types` |
