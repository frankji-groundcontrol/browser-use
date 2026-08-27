# Upstream sync: 25 commits merged, nothing to port to Rust

Date: 2026-08-27 · Practice: [upstream sync and port](../practices/upstream-sync-and-port.md)

## What changed

Merged `upstream/main` into `franky-rust` — 25 commits spanning 2026-08-15 to
2026-08-26, base `f3298c559`. The merge was **conflict-free**.

Upstream released 0.13.8 (Browser Harness 0.1.9) and bumped dependencies. Of the
14 changed files, only three touch `browser_use/`:

| Upstream change | Portable to Rust? |
| --- | --- |
| `ChatBrowserUse` default model `bu-2-0-mini-preview` → `bu-2-0`, with the mini preview now opt-in | **No** — no counterpart exists |
| `filesystem`: escape plain text before ReportLab parses it as markup in generated PDFs | **No** — no counterpart exists |
| `filesystem`: `replace_file` now reports when the target text is absent instead of silently no-op'ing | **No** — no counterpart exists |

The rest is dependency pins, a removed dead CI workflow, docs, examples, skill
files, and the tests covering the two fixes above.

## Why none of it ports

Both candidates were checked against the actual Rust source rather than assumed,
per the practice's rule that a false "not applicable" costs as much as a false
positive:

- **No `ChatBrowserUse` provider.** `bu-llm` contains exactly `openai.rs`,
  `responses.rs`, and `bedrock.rs`. There is no `bu-*` model family anywhere in
  the crate — the only hardcoded model id is a Bedrock Claude one. There is no
  default to flip.
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

If `ChatBrowserUse` or a filesystem tool surface is ever added to the Rust port,
re-check this window: the two filesystem fixes and the model-default change
would become portable and should be revisited from
`f3298c559..upstream/main`.
