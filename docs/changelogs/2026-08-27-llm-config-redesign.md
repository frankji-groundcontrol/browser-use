# One LLM config surface, three wire formats

Date: 2026-08-27 · **Breaking** · Plan: [2026-08-27-rust-chatbrowseruse](../plans/2026-08-27-rust-chatbrowseruse/2026-08-27-rust-chatbrowseruse.md)

## What changed

The LLM environment surface is now four variables, and `BROWSER_USE_LLM_API` is
the only thing that selects a backend.

| Variable | Meaning |
| --- | --- |
| `BROWSER_USE_LLM_BASE_URL` | Endpoint base, used exactly as given |
| `BROWSER_USE_LLM_API_KEY` | Credential |
| `BROWSER_USE_LLM_API` | `openai-responses` \| `openai-chat` \| `anthropic-messages` \| `bedrock` |
| `BROWSER_USE_LLM_MODEL` | Model id (required) |

Plus optional `BROWSER_USE_LLM_TEMPERATURE` (0.7) and
`BROWSER_USE_LLM_MAX_TOKENS` (4096).

**Removed entirely:** `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `ANTHROPIC_API_KEY`,
`ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `MODEL_PROVIDER`,
`BROWSER_USE_OPENAI_API`, `BROWSER_USE_API_KEY`.

## Why

Two defects, not merely untidiness:

1. **The Anthropic path never spoke Anthropic.** `ANTHROPIC_BASE_URL` still sent
   an OpenAI-shaped body to `{base}/chat/completions`. It worked only because the
   gateways in use serve both protocols; pointed at `api.anthropic.com` it failed.
2. **Credential presence doubled as backend selection.** Exporting
   `OPENAI_API_KEY` for an unrelated tool silently changed which model the agent
   used.

## Anthropic Messages, implemented properly

Not a header swap. `x-api-key` instead of bearer auth, the required dated
`anthropic-version` header, `system` hoisted out of the message array into the
top-level field, mandatory `max_tokens`, and images converted from OpenAI
`image_url` data URLs into `{type:"image", source:{type:"base64", …}}` blocks.
Response text blocks are concatenated; non-text blocks (thinking, tool use) are
skipped rather than erroring.

## Base URL: exact first, `/v1` as fallback

The configured URL is used verbatim for the first request. Only if that route
404s — or returns 200 with an HTML landing page — is the alternate root tried
once. This inverts the previous eager `/v1` rewrite, so the URL you set is the
URL used.

The fallback still earns its keep. Probing the deployed gateway:

| Route | Result |
| --- | --- |
| `/v1/responses` | 200 JSON |
| `/responses` | 200 JSON |
| `/v1/chat/completions` | 200 JSON |
| `/chat/completions` | **200 HTML** (the console page) |

A bare base URL with `openai-chat` would land on that HTML page, which is exactly
the case the fallback recovers. Configs now pin `/v1` so the first attempt hits.

## Structure

`bu-llm` was one ~900-line `openai.rs`. Split by responsibility:

| File | Owns |
| --- | --- |
| `config.rs` | `LlmConfig`, `LlmApi`, env loading, URL policy |
| `client.rs` | HTTP, retry/backoff, route fallback, dispatch |
| `openai.rs` | OpenAI request/response bodies |
| `anthropic.rs` | Anthropic Messages bodies |

`OpenAiChatClient`/`OpenAiChatConfig`/`OpenAiApiStyle` became
`LlmClient`/`LlmConfig`/`LlmApi`; `LlmProvider::OpenAi` became
`LlmProvider::Http`.

## Verification

- **43 `bu-llm` tests**, including wire-level proof that the exact URL is tried
  first with *no* fallback attempt when it works, that both 404 and an HTML body
  trigger exactly one retry against the alternate root, that a total failure
  names both URLs and the variable to fix, and that Anthropic sends `x-api-key`
  + `anthropic-version` while OpenAI sends bearer.
- `clippy --all-targets -D warnings` clean on default **and** `live-chrome`.
- **End-to-end against the live gateway** through the migrated config:
  `browser_extract_content` read a page and answered correctly.
- `grok mcp doctor browser-use` and `qoder mcp list`: connected, 19 tools.
- All **10 agent configs across both hosts** migrated by script, backed up as
  `*.bak-llmenv`.

Not verified: the Anthropic path against a real Anthropic endpoint — no key is
available here. Its coverage is the mock server plus the documented contract.

## Follow-up

- Flaky live-Chrome tests, confirmed pre-existing:
  [2026-08-27-flaky-live-chrome-tests](../issues/2026-08-27-flaky-live-chrome-tests.md).
- Bedrock now selects via `BROWSER_USE_LLM_API=bedrock` and takes its model from
  `BROWSER_USE_LLM_MODEL`; it remains feature-gated and is not exercised here.
