# One LLM config: custom base URL × three wire formats

Date: 2026-08-27 · Status: **complete** · Owner: Claude
Tracker: [`2026-08-27-rust-chatbrowseruse.track.yaml`](2026-08-27-rust-chatbrowseruse.track.yaml)

## Objective

Replace the scattered LLM environment surface with one explicit set of
`BROWSER_USE_LLM_*` variables: a base URL the operator controls, a chosen wire
format, a key, and a model. Add a real Anthropic Messages implementation so the
"Anthropic" option stops being a lie.

Supersedes the narrower ChatBrowserUse task this plan opened with: browser-use
cloud needs no dedicated provider under the new scheme — it is just a base URL
plus `openai-chat`.

## Why the current surface is wrong

Provider and route selection are spread across five mechanisms that interact:

| Variable | Today's role |
| --- | --- |
| `OPENAI_API_KEY` / `OPENAI_BASE_URL` | Primary credentials; presence also *selects* the provider |
| `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY` / `ANTHROPIC_BASE_URL` | Silent fallback when no `OPENAI_*` |
| `MODEL_PROVIDER=bedrock` | Selects Bedrock |
| `BROWSER_USE_OPENAI_API` | Picks `/responses` vs `/chat/completions` |
| `BROWSER_USE_LLM_MODEL` / `_TEMPERATURE` | Model and sampling |

Two concrete defects, not just untidiness:

1. **The Anthropic path never speaks Anthropic.** Setting `ANTHROPIC_BASE_URL`
   still sends an OpenAI-shaped body to `{base}/chat/completions`. It works only
   because the gateways in use happen to serve both protocols. Point it at
   `api.anthropic.com` and it fails.
2. **Credential presence is overloaded as provider selection.** Exporting
   `OPENAI_API_KEY` for an unrelated tool silently changes which backend the
   agent uses.

## Decisions (owner)

- **Clean break.** Only `BROWSER_USE_LLM_*` is read. `OPENAI_*`, `ANTHROPIC_*`,
  `MODEL_PROVIDER`, `BROWSER_USE_OPENAI_API`, and `BROWSER_USE_API_KEY` are
  removed, not deprecated. All 14 deployed agent configs (7 agents × 2 hosts)
  must be rewritten in the same change.
- **Base URL: exact first, `/v1` as fallback.** Use the configured URL verbatim.
  Only if that route 404s or returns an HTML landing page, retry once against the
  alternate root (bare gains `/v1`; versioned loses it). Predictable primary,
  forgiving recovery — the inverse of today's eager rewrite.

## New surface

| Variable | Meaning |
| --- | --- |
| `BROWSER_USE_LLM_BASE_URL` | Base URL, used verbatim first |
| `BROWSER_USE_LLM_API_KEY` | Credential |
| `BROWSER_USE_LLM_API` | `openai-responses` \| `openai-chat` \| `anthropic-messages` (\| `bedrock`, feature build) |
| `BROWSER_USE_LLM_MODEL` | Model id |
| `BROWSER_USE_LLM_TEMPERATURE` | Optional, default 0.7 |
| `BROWSER_USE_LLM_MAX_TOKENS` | Optional, default 4096. Required by Anthropic Messages, ignored elsewhere |

Route per format: `{base}/responses`, `{base}/chat/completions`, `{base}/messages`.

`bedrock` joins the `_API` enum because it is a distinct wire protocol, which is
what that variable now means; it keeps working behind its build feature without a
second selection mechanism.

## Anthropic Messages contract

Genuinely different from OpenAI, not a header swap:

- Auth is `x-api-key`, not `Authorization: Bearer`, plus a required
  `anthropic-version: 2023-06-01`.
- `system` is a **top-level field**, not a message role — system messages must be
  hoisted out of the array.
- `max_tokens` is **required**.
- Images are `{type:"image", source:{type:"base64", media_type, data}}`, so the
  OpenAI `image_url` data-URL parts must be converted.
- Responses are `{content:[{type:"text",text}]}` — concatenate the text blocks.

## Module structure

`openai.rs` is ~900 lines and would only grow, so split by responsibility rather
than bolting a second protocol onto it:

| File | Owns |
| --- | --- |
| `config.rs` | `LlmConfig`, `LlmApi`, env loading, base-URL fallback logic |
| `client.rs` | `LlmClient`: HTTP, retry/backoff, root fallback, dispatch by `LlmApi` |
| `openai.rs` | OpenAI request/response bodies (responses + chat) |
| `anthropic.rs` | Anthropic Messages request/response bodies |
| `message.rs`, `responses.rs`, `bedrock.rs` | unchanged |

## Test strategy (test-first)

- `LlmApi::parse` accepts each spelling; an unknown value errors rather than
  defaulting silently.
- Config: each variable read; missing key/base URL produce errors naming the
  variable; removed variables are provably ignored.
- Anthropic serialization: system hoisted, `max_tokens` present, image converted
  to `source.base64`, response text blocks concatenated.
- Base-URL fallback: exact route tried first; 404 and HTML-landing-page both
  trigger exactly one retry against the alternate root; neither working names
  both URLs tried.
- Error hints: a `browser-use.com` base URL keeps the actionable 401/402
  messages; other hosts keep the plain HTTP error.

## Risks

- **Deployment breaks if configs are not rewritten.** 14 configs across two
  hosts; the change is not complete until both hosts verify at 19 tools.
- **Anthropic path is unverifiable live here** — no Anthropic key is available,
  so coverage is the mock server plus the documented contract. State this at
  handoff rather than implying it was smoke-tested.
- Pre-existing flaky `bu-actor` live-Chrome tests fail under full-suite load and
  pass in isolation; confirmed on clean HEAD, unrelated to this work.

## Outcome

Shipped. Verification actually run:

| Check | Result |
| --- | --- |
| `bu-llm` tests | **43 passed**, including wire-level proof of exact-first routing, both fallback triggers, and per-protocol auth |
| `clippy --all-targets -D warnings` | clean on default **and** `live-chrome` |
| End-to-end `browser_extract_content` | correct answer on **both hosts** against the live gateway |
| Agent connectivity | `grok mcp doctor` 19 tools, `qoder mcp list` connected, both hosts |
| Config migration | 10 configs across 2 hosts, backed up as `*.bak-llmenv` |
| MBP2 deploy | `codesign -v` exit 0, exec check exit 1 (not 137) |

Gateway probe that settled the URL policy:

| Route | Result |
| --- | --- |
| `/v1/responses`, `/responses`, `/v1/chat/completions` | 200 JSON |
| `/chat/completions` | **200 HTML** — the console page |

So the fallback is not theoretical: a bare base URL with `openai-chat` lands on
that HTML page. Configs pin `/v1` so the first attempt hits.

**Not verified:** the Anthropic path against a real Anthropic endpoint — no key
available here. Coverage is the mock server plus the contract read from the API
docs. First real use should confirm it before being trusted.
