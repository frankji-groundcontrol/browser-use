# Upstream sync: 25 commits merged, nothing to port to Rust

Date: 2026-08-27 · Practice: [upstream sync and port](../practices/upstream-sync-and-port.md)

## What changed

Merged `upstream/main` into `franky-rust` — 25 commits spanning 2026-08-15 to
2026-08-26, base `f3298c559`. The merge was **conflict-free**.

Upstream released 0.13.8 (Browser Harness 0.1.9) and bumped dependencies. Of the
14 changed files, only three touch `browser_use/`:

| Upstream change | Portable to Rust? |
| --- | --- |
| `ChatBrowserUse` default model `bu-2-0-mini-preview` → `bu-2-0`, with the mini preview now opt-in | **No** — no provider class exists to hold a default; the Rust model comes from `BROWSER_USE_LLM_MODEL` |
| `filesystem`: escape plain text before ReportLab parses it as markup in generated PDFs | **No** — no counterpart exists |
| `filesystem`: `replace_file` now reports when the target text is absent instead of silently no-op'ing | **No** — no counterpart exists |

The rest is dependency pins, a removed dead CI workflow, docs, examples, skill
files, and the tests covering the two fixes above.

## Why none of it ports

Both candidates were checked against the actual Rust source rather than assumed,
per the practice's rule that a false "not applicable" costs as much as a false
positive:

- **No `ChatBrowserUse` provider class — but the endpoint is not unreachable.**
  `bu-llm` contains exactly `openai.rs`, `responses.rs`, and `bedrock.rs`, so
  there is no constructor default to flip. Worth being precise about *why* that
  is fine: `ChatBrowserUse` POSTs `{base_url}/v1/chat/completions` with
  `Authorization: Bearer`, i.e. the OpenAI wire format, against
  `https://llm.api.browser-use.com`. The Rust generic client POSTs
  `{base}/chat/completions` with the same auth, so browser-use's cloud LLM is
  reachable **by configuration** —
  `OPENAI_BASE_URL=https://llm.api.browser-use.com/v1`,
  `BROWSER_USE_LLM_MODEL=bu-2-0` — with no new provider code.

  What a dedicated Rust provider would add over that: the friendly 401/402
  messages ("`BROWSER_USE_API_KEY` is invalid", "credits exhausted", each with a
  billing link) instead of a generic HTTP error, and `session_id` passthrough.
  It would *not* add retry parity — `openai.rs` already retries 429/5xx with
  exponential backoff honoring `Retry-After` — and it would not add structured
  output, because the Rust agent does not use the provider's structured-output
  API at all: `bu-agent::action::parse_output` strips code fences and parses JSON
  from the response text. That also sidesteps a real incompatibility, since
  browser-use's gateway expects a non-standard `output_format` key rather than
  OpenAI's `response_format`.
- **No filesystem subsystem.** No Rust crate implements file read/write/replace.
  `bu-tools` exposes the 18 `browser_*` tools, and `bu-mcp` adds
  `retry_with_browser_use_agent`; none of them touch files. Neither the PDF
  escaping bug nor the `replace_file` silent-no-op can occur, because the code
  that has them was never ported.

This is a real result, not a skipped step: the Rust port deliberately covers the
MCP/browser surface, and upstream's activity this window fell entirely outside
it.

## Verification

- Merge: clean, no conflicts.
- `cargo build --release -p bu-core`: green.
- Routers survived at 100 / 108 lines; `check_target_routers.py` passes.
- Upstream did **not** touch `AGENTS.md` or `CLAUDE.md` this window, so the
  conflict introduced by
  [the router restructure](2026-08-26-mcp-rollout-and-docs-records.md) was not
  exercised yet. It still will be on the first upstream edit to those files.

## Follow-up

- If a filesystem tool surface is ever added to the Rust port, revisit this
  window — both filesystem fixes would become portable from `f3298c559`.
- A dedicated `ChatBrowserUse` provider in `bu-llm` is **optional**, not a gap:
  it would buy actionable 401/402 messages and `session_id` passthrough. Worth
  doing only if the fork actually points at browser-use's cloud LLM; the current
  deployment uses a private OpenAI-compatible gateway.
- **Scope caveat.** This assessment covers one 11-day window (25 commits). It
  does not establish that the Rust port is at parity with Python overall — only
  that upstream's activity *since the last sync* fell outside the ported
  surface. A standing parity audit is a separate exercise.
