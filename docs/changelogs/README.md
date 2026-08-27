# Change records

Human-readable records of meaningful repository changes — what changed, why, and
how it was verified — so a future maintainer does not have to reconstruct intent
from the diff.

Use a single dated file, `YYYY-MM-DD-title.md`, for an ordinary change. Use a
dated folder, `YYYY-MM-DD-title/`, with an `index.md` plus focused child files
(`summary.md`, `structure.md`, `verification.md`, `migration-notes.md`) when the
change is large enough that one file would be hard to scan.

Keep entries about the change, not the task transcript. Link the related
[plan](../plans/README.md), [issue](../issues/README.md),
[learning](../learning/README.md), [practice](../practices/README.md), and
[usage](../usage/README.md) records instead of restating them. Redact private
identifiers — tokens, API keys, gateway hosts, project refs, account IDs, and
local runtime paths — unless publication was explicitly approved.

## Index

- [2026-08-27 — One LLM config surface, three wire formats](2026-08-27-llm-config-redesign.md)
  — **breaking**: `BROWSER_USE_LLM_*` replaces the `OPENAI_*`/`ANTHROPIC_*`/
  `MODEL_PROVIDER` tangle, Anthropic Messages is implemented for real, and the
  base URL is used exactly as given with `/v1` only as a fallback.
- [2026-08-27 — Upstream sync: 25 commits merged, nothing to port to Rust](2026-08-27-upstream-sync-nothing-to-port.md)
  — upstream's only code changes this window landed in `ChatBrowserUse` and the
  filesystem module, neither of which the Rust port implements; both refuted
  against the real source rather than assumed.
- [2026-08-26 — Multi-agent MCP rollout and docs record system](2026-08-26-mcp-rollout-and-docs-records.md)
  — registered the Rust MCP server with Kimi Code, Qoder, and Grok across two
  hosts, corrected the deploy and setup docs, and added the changelog, handoff,
  and recording-guardrail surfaces this fork was missing.
