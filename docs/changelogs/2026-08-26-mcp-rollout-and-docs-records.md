# Multi-agent MCP rollout and docs record system

Date: 2026-08-26 · Plan: [2026-08-26-docs-structure-cleanup](../plans/2026-08-26-docs-structure-cleanup/2026-08-26-docs-structure-cleanup.md)

## What changed

**MCP rollout.** `browser-use-rs --mcp` is now registered with three more coding
agents — Kimi Code, Qoder, and Grok — on both the primary host and the secondary
host, taking the fleet from four agents to seven. Grok and Qoder were registered
through their own `mcp add` CLIs; Kimi Code has no `mcp` subcommand, so its
entry was merged into `~/.kimi-code/mcp.json` with `jq`, preserving sibling
servers.

**Binary sync.** The secondary host's deployed `~/.local/bin/browser-use-rs` was
stale against its own build tree and was redeployed from `HEAD`. Full evidence in
[issues/2026-08-26-installed-binary-drift-from-build-tree.md](../issues/2026-08-26-installed-binary-drift-from-build-tree.md).

**Docs corrections.** Two docs had drifted from the implementation:

- The tool count was documented as 18 (and 17 in the deploy practice); it is
  **19**. `browser_select_option` is a third fork-only addition alongside
  `browser_set_viewport` and `browser_read_clipboard`.
- Both docs described a four-agent fleet.

**Record system.** This fork's `docs/` was missing the change-record and handoff
surfaces, so nothing nudged an agent to record anything:

- Added `docs/changelogs/` (this folder) and `docs/HANDOFF.md`.
- Installed the docs-recording guardrail: `scripts/check-docs-recorded.sh`,
  `config/git-hooks/pre-commit` (warns when source changes carry no changelog),
  `config/git-hooks/commit-msg` (a `[checkpoint]` commit must stage the
  changelog + active plan + handoff), and `core.hooksPath` pointed at the
  tracked hooks directory so Codex, Claude, and humans all get the same check.
- Rebuilt `docs/plans/README.md` as a real cross-task status board, and stated
  the plan-record shape (flat / folder / tracked) so the next agent does not
  have to infer it.
- Added a short **Working rules** block to the fork-owned region of both
  routers — plan-first, coding baseline, modular-implementation and chunky-file
  handling, English-only prose, privacy redaction, link/index verification.
  These were previously implicit.

**Router restructure (owner-directed).** Both routers were then reduced to
genuine routers, accepting the upstream merge-conflict cost:

| Router | Before | After | Where the content went |
| --- | --- | --- | --- |
| `AGENTS.md` | 1070 | **100** | [`docs/usage/library/`](../usage/library/README.md) — the 985-line embedded `<browser_use_docs>` handbook, split into quickstart / agent / browser / tools / production / support |
| `CLAUDE.md` | 211 | **108** | [Python code style and change workflow](../practices/2026-08-26-python-code-style-and-change-workflow.md); architecture and command sections became links |

Every content line of the extracted block is preserved; only the
`</browser_use_docs>` wrapper tag is gone. Both routers now carry a
merge-resolution note: keep the router thin, apply upstream's content change to
the matching `docs/usage/library/` page. Reasoning and the decision trail are in
[learning/2026-08-26-fork-router-editable-region.md](../learning/2026-08-26-fork-router-editable-region.md).

## Affected files

| Path | Change |
| --- | --- |
| `docs/usage/tools/mcp-multi-agent-setup.md` | 19 tools, 7 agents, per-agent commands, two verification traps |
| `docs/practices/deploy-browser-use-rs.md` | tool count, agent count, second-host drift check |
| `docs/practices/2026-08-26-technical-lead-dev-worker-operations.md` | new — role split |
| `docs/practices/2026-08-26-docs-recording-guardrail.md` | new — guardrail record |
| `docs/issues/2026-08-26-installed-binary-drift-from-build-tree.md` | new |
| `docs/learning/2026-08-26-fork-router-editable-region.md` | new |
| `docs/changelogs/`, `docs/HANDOFF.md` | new surfaces |
| `docs/plans/README.md`, `docs/index.md`, aspect READMEs | indexes and status board |
| `CLAUDE.md`, `AGENTS.md` | fork-owned `## Documentation` block only |
| `scripts/check-docs-recorded.sh`, `config/git-hooks/*` | guardrail |

## Verification

- `grok mcp doctor browser-use` on both hosts: handshake OK, **19 tools**.
- `qoder mcp list` on both hosts: `browser-use … Connected`.
- Kimi Code on both hosts: one-shot prompt returned 18 `browser_*` tool names
  (the 19th, `retry_with_browser_use_agent`, does not carry the prefix).
- Secondary host after redeploy: installed and build-tree hashes match,
  `codesign -v` exits 0, exec check exits 1 (not 137).
- `check_target_routers.py` passes against this repo.
- Secret sweep over `docs/` for keys, bearer tokens, and gateway hosts: clean.

## Follow-up

- The secondary host's Kimi entry attaches to an **ephemeral** CDP port
  (`127.0.0.1:<port>`), which dies whenever that Chrome restarts. Left as-is
  because the endpoint is live and the setup is deliberate.
- The two hosts deploy the binary differently — symlink into `target/release/`
  on the primary, file copy on the secondary. Only the copy can drift. Picking
  one layout is still open; see the issue record.
