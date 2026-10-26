# Translation Cell

Code: `server/translation-cell` · Contract: `types/translation_cell.tyml`

The Translation Cell holds translations, translation context and language
policies of spaces (guilds, workspaces). It is the part other platforms
adopt: a platform keeps its own message store and only

1. streams messages in (`ingest`, `delete_message`),
2. optionally describes its spaces (`put_space`, `put_node`),
3. reads translations (`get_translations`, `poll_events`).

BabelChat's Message Cells use exactly this API.

## Identifiers

Everything is keyed by identifiers the platform chooses:

```
MessageRef { space, conversation, id }    + rev (opaque, changes on edit)
```

- Keys are IDs, not hashes of the text. A text hash cannot key translations:
  the same words translate differently in different contexts, edits would
  orphan old entries, deletions would need the original text, and short texts
  are guessable from their hash.
- `rev` plays the role a hash would: translations record the revision they
  were made from; a different revision means stale.
- The tenant comes from the caller's credentials, never from the request.
  Phase 0 serves one tenant (`local`), and every key already starts with it.

## Ingest: translation starts at once

```
ingest(message)                                      under the space's lock
  ├─ same message and revision already seen? → done (idempotent)
  ├─ detect the source language (text + author's locale)
  ├─ targets = LLM languages − source (and its regional variants)
  ├─ budget left today?
  ├─ build the request: context layers + window snapshot + message
  └─ one write: translation record (pending) + window + job
        └─▶ job: Engine.translate (async, retried on unavailability)
              └─▶ one write: translation record (done) + event + job removed
```

- **N to N.** Each message goes from its source language directly into every
  LLM language of the space, in one engine call. No pivot through English, so
  errors do not compound.
- **Free spaces** have no LLM languages: the record is `done` with no
  translations, but the event still carries `source_lang`, so every client
  starts on-device translation from the same detected language.
- **Jobs are durable.** The request is snapshotted into the job, so a job
  resumed after a crash sends exactly the same prompt.
- **Edits** replace the window entry in place and queue a new job; a result
  for an older revision is discarded.

## Context

Translations use a hierarchy of context, from the space down:

```
space        name, description (or its digest), glossary     put_space
category     title, description                              put_node
channel      title, topic                                    put_node
thread       frozen fork: parent, starter message, last messages before it
summary      what left the conversation's window
window       the last messages of the conversation, originals only
```

- **Root context lives here**, as a translation-ready copy. The platform is
  the source of truth for names and descriptions and re-sends them when they
  change. Descriptions longer than 600 characters are digested by the engine
  (`summarize`), in whatever language they are written.
- **A new thread is never context-free.** Its fork snapshot is taken when the
  thread appears, from the parent's window, and frozen so the thread's prompt
  prefix does not change as the parent moves on.
- **Windows only grow**, then fold: past 40 messages the oldest are folded
  into the summary in one engine call, keeping the newest 15. Prompts built in
  between share a growing prefix, which keeps the engine's prompt cache
  hitting (see [Translation Engine](translation-engine.md)). If folding keeps
  failing, the window is cut at 60 without a summary.

## Language policy

The Translation Cell is the source of truth for each space's
[language policy](languages.md): declared languages, LLM languages, plan
limits, region mode and aliases. Platforms check their own permissions and
then call `put_policy`; the Translation Cell has no notion of roles.

`normalize` and `resolve_viewer` exist for platforms that cannot run
`babel-lang`; Rust callers normalize locally on a policy snapshot.

## Events

Each space has an event log (`translated`, `removed`, `policy`) that Edges
long-poll. Events of a space are ordered; `head` lets a follower start at the
end.

## Budgets and usage

- `daily_budget` caps LLM-translated messages per space per UTC day. Past it,
  messages are marked `budget_exceeded` and clients fall back to on-device
  translation. This is the guard against spam burning the LLM bill.
- `get_usage` reports translated messages and token usage, including cache
  reads and writes as reported by the engine.

## Storage

```
spaces        tenant / space                       SpaceRecord (context, digest, policy, usage, budget, last event)
nodes         tenant / space / node                NodeRecord (node, version, fork)
windows       tenant / space / conversation        WindowRecord (entries, summary)
translations  tenant / space / conversation / id   TranslationRecord (rev, source, status, translations)
events        tenant / space / seq                 TranslationEvent
jobs          seq                                  Job (message, rev, request snapshot)
```

Original text lives only in windows and in queued jobs.
