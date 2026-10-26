# TYML changes made for BabelChat

BabelChat uses a local TYML checkout (`../tyml`) while these changes are
reviewed. They follow two rules:

1. TYML stays minimal and covers 90% of cases.
2. When an existing feature can express something without costing
   performance or safety, use it instead of adding a feature.

**No language feature was added.** All changes are in the code generator
(`src/tyml_api_generator`), are uncommitted in the TYML repository, and the
generator's tests pass (20 tests, 3 of them new).

## Fixes to generated code

| Problem | Effect before | Now |
|---|---|---|
| Struct fields came out in hash-map order | Field order changed on every generation (Rust, TypeScript, Kotlin), so regenerating produced noisy diffs | Declaration order (`general::ordered_elements`), in all three generators |
| Enum literals became Rust identifiers verbatim | `"en-US"` did not compile; `"merged"` produced a lowercase variant | `UpperCamelCase` variants with `#[serde(rename = "...")]`; clashes get a numeric suffix |
| Map types (`{ *: T }`) became empty structs | Map contents were silently dropped on deserialization | `BTreeMap<String, T>`; a named map becomes a type alias; fixed fields plus `*` flatten the rest into `__rest` |
| Field names that are Rust keywords or not identifiers | `ref: string` or `"display-name": string` did not compile | Raw identifiers (`r#ref`) or a sanitized name with `#[serde(rename)]` |
| Recursive named types | Marked as generated only after generating them, so a type referring to itself recursed without end | Marked before generating |
| `JwtValidator::validate` took no `&self` | The signing key had to live in a global | `fn validate<T>(&self, token: &str)`; the router passes the API value |
| The Rust client's `ThrowsOrOtherError` had no `Debug` | Results could not be `unwrap`ped or printed | `#[derive(Debug)]` |

The `JwtValidator` change breaks existing implementations: they need a
`&self` parameter.

## Small additions to generated code

- Structs derive `PartialEq`; enums also derive `Eq` and `Hash` (handy in
  tests and maps).
- Generated crates start with `#![allow(clippy::all)]`, so a workspace-wide
  clippy run does not lint generated code.
- Doc comments are emitted one per line regardless of trailing newlines.

## Expressed with existing features instead

| Need | How |
|---|---|
| Events with different payloads | A `kind` enum and one optional field per payload |
| Streams (event logs, placement watch) | Long-poll functions: `poll_events(after, wait_ms) -> { events, next, head }` |
| Sharing types across services | One file per boundary; a client crate converts to the server crate's types with serde (`babel_common::convert`) |
| Ordered key-value settings | Arrays of pairs (`[LanguageAlias]`) |
| Timestamps | `int` milliseconds |

## Left as is, for later

- Every thrown error is HTTP 400 (401 for bad tokens). Fine while clients
  are generated; worth revisiting for third-party SDK users.
- No `import`: one file per boundary works for now; a few concepts
  (`Translation`) exist in two files and are converted explicitly.
- The generated router does not expose the client address, which per-IP
  rate limiting on login will need.
- The enum, map and keyword fixes are in the Rust generator only; the
  TypeScript and Kotlin generators only got the field-order fix.

## Phase 1 (chat without a UI)

No TYML changes. The SDK needed one shared connection pool for all
interfaces; the generated clients already expose `url` and `client` as
public fields, so it builds them directly instead of calling `new`.
