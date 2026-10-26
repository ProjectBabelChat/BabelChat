# Languages

Code: `crates/babel-lang`, `server/translation-cell/src/policy.rs`,
`server/translation-cell/src/detect.rs`

## Language policy of a space

Each space (guild) has a policy, owned by its Translation Cell:

```
SpacePolicy {
  plan                         free | paid1 | paid2 | paid3
  languages                    declared languages, normalized
  llm_languages                the subset the LLM translates into
  max_languages, max_llm_languages
  region_mode                  merged | distinct
  aliases                      space-specific overrides, e.g. pt -> pt-PT
  translate_regional_variants  translate pt-BR <-> pt-PT in distinct mode
}
```

| Plan | Languages | Of which LLM |
|---|---|---|
| free | 3 | 0 (on-device only) |
| paid1 | 3 | 2 |
| paid2 | 3 | 3 |
| paid3 | 5 | 5 |

Billing does not exist yet: new spaces get the plan from configuration
(`babel-server --plan`), and `set_plan` is the hook billing will call.
Downgrades trim languages that no longer fit.

## Normalization

Every component compares languages through `babel_lang::Normalizer` on the
same policy snapshot. Disagreeing here would split translation keys and shake
prompt prefixes, so the rules live in one crate.

```
normalize("en_GB", policy)
  1. parse      en_GB, en-gb, EN-GB → en-GB (underscores accepted, variants dropped)
  2. fill in    likely script and region from CLDR: zh-TW → zh-Hant-TW
  3. aliases    the most specific matching alias of the space wins
  4. region     merged: the language's representative region   en-GB → en-US
                distinct: the region as written (or the likely one)
  5. render     shortest form: the script only when not the default
                (en-GB, but zh-Hant-TW)
```

- **Regions collapse, scripts never.** `zh-TW` and `zh-CN` look like regional
  variants but are Traditional and Simplified Chinese.
- Representative regions: `en-US`, `pt-BR`, `es-419` (most Spanish speakers
  live in Latin America; Spanish communities can alias `es` to `es-ES`),
  `zh-Hans-CN` (rendered `zh-CN`), `zh-Hant-TW`; CLDR's likely region for the
  rest (`ja-JP`, `fr-FR`...).
- Languages are counted after normalization: in merged mode `en-GB` and
  `en-US` are one language.

## What each reader sees

`babel_lang::resolve_viewer` combines a user's locale and personal setting
with a space's policy:

| Reader's language | Shown |
|---|---|
| the source language | the original |
| an LLM language | the LLM translation (context-aware) |
| declared, not LLM (free spaces) | on-device translation |
| not declared, setting on | on-device translation from the English LLM translation if any, else from the original |
| not declared, setting off | the original |

The setting (`translate_outside_set`) is personal and lives in the Account
service, so it follows the user across guilds and platforms. Edges compute
the result (`LanguageView`) so clients need no tables.

## Source language detection

Text tells the language but rarely the region, and for Chinese not even the
script. `detect.rs` combines detection (whatlang) with the author's locale:

- detection agrees with the author's language → the author's locale
  (a Taiwanese author's Chinese is `zh-Hant-TW`);
- confident detection (≥ 0.5) disagrees → the detected language
  (a Japanese user writing English is writing English);
- unsure → the author's locale; nothing at all → `und`, translated into every
  LLM language.

The detected language travels with every translation event, so all clients
start on-device translation from the same source.

## On-device translation (free tier)

Clients will bundle Bergamot (Firefox's engine) and Mozilla's models. Those
models pair languages with English, so translation is N → English → N:

- one hop when the source or the target is English, two otherwise;
- readers need `source → en` and `en → own language` models, downloaded on
  first use;
- when the guild has an English LLM translation, clients start from it
  instead of the original: one hop, and context-aware. Edges always send the
  English translation along for this reason.

Client-side translation is not part of Phase 0.
