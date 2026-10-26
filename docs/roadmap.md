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

## Phase 1 — split it up

- HTTP clients behind the internal traits; each service as its own binary.
- Internal network: WireGuard plus mutual TLS; service credentials; tenant
  from credentials in the Translation Cell.
- Egress proxy for all outbound traffic.
- Backups (`BackupEngine` + log shipping) and a standby per cell.
- Guild migration (export, catch-up, fenced switch-over).
- WebSocket gateway.
- Log trimming.

## Phase 2 — real translation

- Validate the OpenAI-compatible backend against llama.cpp / vLLM with
  Hy-MT2 and Qwen3; tune window and summary sizes with real usage numbers.
- Space-affinity routing for engine replicas.
- Reference channels (owner-chosen descriptions) as context.
- Translation Cell as a standalone, self-hostable product for other
  platforms; Server SDK documentation.

## Phase 3 — the product

- BabelChat Cloud engine; billing setting plans; Discord account linking.
- Managed Translation Cells for platforms that do not want to host one.
- Desktop client (GPUI + MD3E) with on-device Bergamot translation.

## Open questions

- Retention of translation events and windows per plan.
- Whether free guilds get an English LLM translation to pivot from (today
  they translate from the original on device).
