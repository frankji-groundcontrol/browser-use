# Handoff

## What the team is trying to finish

This fork (`franky-rust`) maintains a Rust reimplementation of browser-use's MCP
server, `browser-use-rs`, which is the browser-automation backend for every
coding agent on two developer machines. Two threads are live:

- **The Rust rewrite itself** — the long-running port, governed by
  [2026-07-05-rust-rewrite](plans/2026-07-05-rust-rewrite/index.md).
- **Operating the deployed server** — keeping the binary and its MCP
  registrations correct across both hosts, governed by
  [usage/tools/mcp-multi-agent-setup.md](usage/tools/mcp-multi-agent-setup.md)
  and [practices/deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md).

Current status for every task is on the board:
[plans/README.md](plans/README.md).

## What is true now

The server is registered with **seven** agents (Claude Code, Codex CLI, OpenCode,
Hermes, Grok, Qoder, Kimi Code) on both hosts, all verified connected at 19
tools. Both hosts sit on the same commit with clean trees, and the
docs-recording guardrail is active on both.

**The LLM environment surface changed on 2026-08-27 and is a breaking change.**
Configure the model with `BROWSER_USE_LLM_BASE_URL`, `_API_KEY`, `_API`
(`openai-responses` | `openai-chat` | `anthropic-messages` | `bedrock`), and
`_MODEL`. `OPENAI_*`, `ANTHROPIC_*`, `MODEL_PROVIDER`, `BROWSER_USE_OPENAI_API`,
and `BROWSER_USE_API_KEY` are no longer read at all. All 10 agent configs on both
hosts are migrated (backups at `*.bak-llmenv`); any *new* config must use the new
names or the server will refuse to start with a message naming the variable.

Note for any **third** clone: the guardrail's `core.hooksPath` is per-clone local
config and does **not** arrive with a pull. The hook files will be present but
inert until someone runs `git config core.hooksPath config/git-hooks`.

Two divergences a newcomer would otherwise trip over:

- The hosts deploy the binary **differently**. The primary symlinks
  `~/.local/bin/browser-use-rs` into `rust/target/release/`; the secondary keeps
  a file copy. Only the copy can go stale, and git reports nothing when it does —
  see [the drift issue](issues/2026-08-26-installed-binary-drift-from-build-tree.md).
  Hash the deployed file against its own host's build tree before assuming a
  sync is done.
- The secondary host's Kimi entry attaches to a running Chrome over an
  **ephemeral** CDP port rather than launching its own. It works today and is
  deliberate, but it breaks whenever that Chrome restarts.

## What needs to happen next

1. **Verify the Anthropic Messages path against a real endpoint.** It is
   implemented and unit-tested against a mock, but no Anthropic key was
   available here, so it has never spoken to `api.anthropic.com`. Set
   `BROWSER_USE_LLM_API=anthropic-messages` with a real key and run
   `browser_extract_content` once.
   Done when: a live Anthropic call returns an answer, or the gap is fixed.

2. **Decide on one binary deploy layout across both hosts.** The primary
   symlinks `~/.local/bin/browser-use-rs` into `rust/target/release/`; the
   secondary keeps a file copy. The symlink cannot drift but breaks on
   `cargo clean`; the copy survives a clean but goes stale silently, which is
   what already bit us once.
   Done when: both hosts use the same layout and
   [deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md) states which
   and why.

3. **Give the secondary host's Kimi entry a stable browser.** It attaches over
   an ephemeral CDP port that dies whenever that Chrome restarts.
   Done when: the entry either launches its own browser or points at an
   endpoint that survives a restart.

4. **Resume the Rust rewrite** at whatever its plan's tracker names as the next
   step. Done when: that step's own exit evidence is satisfied.

## How to pick up the work

Update the row on [plans/README.md](plans/README.md) and the detailed plan
together whenever work starts, blocks, or completes — the board owns cross-task
status, the plan owns scope and decisions, and a tracked plan's `.track.yaml`
owns step-level state.

Constraints to preserve:

- **The routers now conflict with upstream by design.** `AGENTS.md` (1070 → 100
  lines) and `CLAUDE.md` (211 → 108) had their embedded documentation extracted
  into [`docs/usage/library/`](usage/library/README.md) and
  [practices](practices/2026-08-26-python-code-style-and-change-workflow.md) on
  2026-08-26, at the owner's direction. When merging `upstream/main`, resolve by
  keeping the router thin and applying upstream's content change to the matching
  `docs/usage/library/` page — do **not** paste the handbook back into the
  router. For any *other* inherited file (CI, linters), check
  `git diff upstream/main -- <file>` and ask before restructuring. See
  [the router lesson](learning/2026-08-26-fork-router-editable-region.md).
- **Never `cp` over the live binary** — macOS SIGKILLs it. Fresh inode, then
  re-sign; the deploy practice has the sequence.
- **Redact secrets in records.** Agent MCP configs hold live API keys and
  gateway hosts. Note that `qoder mcp get <name>` prints them in plaintext;
  prefer `qoder mcp list`.
