# Browser Use cloud support lost two things in the config redesign

Date: 2026-08-27 · Scope: `bu-llm` — Browser Use cloud LLM (`llm.api.browser-use.com`)
Status: **closed, not applicable** (2026-08-27) — see Resolution.

## Resolution: this deployment does not use Browser Use cloud

The `bu-*` models are ordinary language models behind an OpenAI-compatible
endpoint, marketed by Browser Use as *faster and cheaper* for browser automation
rather than more capable. This deployment runs a frontier general model
(`gpt-5.6-sol`) on a private gateway, which trades that speed for stronger
reasoning — so there is nothing to gain by pointing at `llm.api.browser-use.com`.

Confirmed by exercising the most demanding LLM path on the configured model:
`retry_with_browser_use_agent` completed a two-step task (click a button, then
read the value it revealed) and answered correctly. That path requires strict
JSON action emission plus multi-step reasoning; `browser_extract_content` was
verified repeatedly alongside it on both hosts.

The gaps below are therefore **not defects to fix** but a record of what would
need doing *if* someone ever pointed this fork at Browser Use cloud. Left here
for that reason, not as outstanding work.

## Context

A dedicated `crates/bu-llm/src/browser_use.rs` was written on 2026-08-27 (model
aliases, endpoint defaults, actionable 401/402 errors), then **deleted hours
later** during the
[`BROWSER_USE_LLM_*` redesign](../changelogs/2026-08-27-llm-config-redesign.md).
That deletion was correct in the main: under the new scheme Browser Use cloud is
just a base URL plus `openai-chat`, so a provider module would have been dead
weight.

Two pieces did not survive the move, and neither was noticed at the time.

## Gap 1 — model alias normalization was dropped

Python's `ChatBrowserUse` normalizes `bu-latest` to the concrete `bu-2-0` before
sending, and rejects an unrecognized bare id locally with the valid-alias list.
The deleted module reproduced both. Nothing does now:

```
$ grep -rn "normalize_model\|bu-latest\|MODEL_ALIASES" crates/
(no matches)
```

So `BROWSER_USE_LLM_MODEL=bu-latest` is now forwarded verbatim, and a typo like
`bu-2` reaches the gateway instead of failing locally with a list of what is
valid.

**Severity: unknown, and that is the point.** Python normalizing client-side
suggests the gateway may not resolve `bu-latest` itself — but that is an
inference, not a measurement. It needs one live call to settle.

## Gap 2 — the 401/402 hint lost its integration test

The hint survived, as `status_hint()` in `client.rs`, keyed off the base URL
containing `browser-use.com`. It has two unit tests and they pass.

But the deleted `openai.rs` also had **HTTP-level** tests
(`browser_use_401_names_the_key_variable`, `browser_use_402_explains_credits`,
`generic_provider_keeps_the_plain_error`) that drove a mock server and proved the
message actually reaches the caller through `chat()`. Those were not carried into
`client.rs`. The surviving line —

```rust
return Err(match status_hint(&self.config.base_url, status.as_u16()) { … });
```

— is now unverified. The function is tested; the wiring is not. This is a
coverage regression introduced by the rewrite, not a pre-existing gap.

## Why re-adding gap 2's test is awkward

The mock server binds `127.0.0.1:<port>`, and `status_hint` matches on the base
URL's host, so a loopback test URL can never contain `browser-use.com`. The
host-matching and the connection target are the same string.

Fix options, in preference order:

1. Compute the hint decision once on `LlmConfig` at construction (a boolean or
   small enum), so a test can set it directly while production still derives it
   from the host. This is essentially the `ProviderKind` field the redesign
   removed — removing it is what created the untestable seam.
2. Have `chat()` take the hint resolver as an injectable dependency.
3. Accept the gap and document it. Weakest: the wiring is exactly where a
   refactor would silently break it.

## Live endpoint: verified up to the auth boundary

Updated 2026-08-27 after an actual probe. No valid Browser Use key exists here,
but pointing the client at the real service with a deliberately invalid one is
still informative, and it passed:

```
POST https://llm.api.browser-use.com/v1/chat/completions
  -> HTTP 401 {"detail":"Invalid API key. Get your API key at ..."}
  -> surfaced as "BROWSER_USE_LLM_API_KEY is invalid or missing.
     Get a new key at https://cloud.browser-use.com/new-api-key"
```

That establishes, against the real service rather than a mock:

- the route is right — it was reached directly, with no 404 and no fallback retry;
- the auth **shape** is accepted — the server evaluated the credential and
  rejected the key, not the request format;
- the 401 hint fires end-to-end, not merely in the `status_hint` unit test — so
  **gap 2's missing integration coverage is now partly answered by live
  evidence**, though still not by a repeatable test;
- the responder is genuinely Browser Use — its own error text arrived alongside
  ours, so this was not a proxy or a landing page.

**Still unproven:** a successful 200 completion parsing. That path is
`openai-chat` response handling, which is heavily unit-tested and runs against
the deployed gateway continuously, so the residual risk is low — but it is not
zero and should not be described as verified.

## Acceptance for closing this

- `bu-latest` resolves to `bu-2-0` (client-side, or measured to be unnecessary),
  and an invalid bare model id fails locally naming the valid aliases.
- An HTTP-level test proves a 401 and a 402 surface their guidance through
  `chat()`.
- One live `browser_extract_content` against `llm.api.browser-use.com` succeeds
  **with a valid key** (the 401 path is already confirmed live).

Related: [key storage](../changelogs/2026-08-27-key-storage-dotenv-keychain.md),
[the lesson on unverified integrations](../learning/2026-08-27-working-by-accident-is-not-working.md)
— which applies squarely here.
