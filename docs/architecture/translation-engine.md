# Translation Engine

Code: `server/engine` · Contract: `types/engine.tyml`

The engine turns a request into translations. It is stateless — nothing sent
to it is stored — so it can be shared, replicated, or run by someone else.

## One API, several implementations

```
Engine API (types/engine.tyml)
 ├─ babel_engine::Engine over a Backend          in-process or served by `babel-engine`
 │    ├─ MockBackend           development: fake translations, simulated prompt cache
 │    └─ OpenAiCompatBackend   self-hosting: vLLM, llama.cpp, Ollama, LM Studio...
 ├─ babel_engine::RemoteEngine                    any Engine API server over HTTP
 └─ BabelChat Cloud (closed)                      tuned prompts and models, same API
```

Translation Cells hold an `Arc<dyn Engine>` and cannot tell these apart.

**Self-hosting path:** run `babel-engine --backend openai` next to a local
model server and point `babel-server --engine-url` at it (see
[development](../development.md)). For models, Tencent Hy-MT2 (Apache-2.0,
33 languages) or Qwen3 are good starting points.

## The request

```
TranslateRequest {
  layers:  [ContextLayer]    space, conversation, fork, summary — broad to narrow
  window:  [SourceMessage]   recent originals, oldest first
  message: SourceMessage
  source_lang, targets       N to N: every target straight from the source
}
```

Translation Cells assemble context; engines decide how to render it. The
same `ContextLayer { id, version }` always carries the same content, so an
engine may cache by it.

## Prompt layout and caching

`prompt.rs` renders every backend's prompt, from what changes least to what
changes most, with a cache breakpoint after each level:

```
system  L0 instructions               shared by every space           breakpoint 1
user    L1 space layers               shared by a space               breakpoint 2
        L2 conversation layers        shared by a conversation        breakpoint 3
        L3 window, one block per msg  grows by appending              breakpoint 4 (last message)
        L4 message and targets        changes every request
```

Rules that keep the cache hitting:

- **Deterministic rendering.** A changed byte invalidates every cached prefix
  after it, so nothing volatile (time, IDs) is rendered, and JSON is written
  from structs with a fixed field order.
- **One block per window message.** Prompt caches (Claude's at least) match at
  block boundaries. When a message is appended, the previous request's last
  breakpoint is still a block boundary, so its entry is read back.
- **Windows only grow, then fold** (see [Translation Cell](translation-cell.md)).
  A sliding window would change the prefix on every message and never hit.
- **A new thread still hits L0 and L1**: caching is per prefix, not per
  thread, so the thread shares the space's prefix with every other
  conversation.

Content is embedded as JSON strings, so message text can never close a
section or pose as an instruction. The engine also rejects outputs that miss
a target or exceed four times the source length (plus slack) — the usual
signature of a model following instructions hidden in a message.

### The mock measures what the real thing would cost

`MockBackend` renders the same prompt and simulates a prefix cache on it:
entries written at breakpoints, reads matched at any block boundary within 20
blocks before a breakpoint, five-minute TTL refreshed on hits, nothing cached
below 512 tokens. Its `Usage` (input, cache read, cache write, output tokens)
lets the window size and folding be tuned before any model is involved. Token
counts are estimates (about three characters per token).

## OpenAI-compatible backend

- `POST {base_url}/chat/completions` with the L0 instructions as the system
  message and the rest as one user message; `response_format: json_object`
  unless disabled.
- These servers cache prompt prefixes automatically. With several replicas,
  route by space (consistent hashing on the space ID) so each space's prefix
  stays warm on one replica.
- Cached tokens reported by the server (`prompt_tokens_details.cached_tokens`)
  show up as cache reads in `Usage`.
- Tested against a fake server only; not yet against a real model server.

## Summaries

`summarize` serves two purposes: digests of long descriptions (into the space
layer) and rolling summaries of messages that left a window. Summaries are
written in the language of the data unless one is requested.
