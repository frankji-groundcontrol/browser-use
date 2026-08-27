# Docs structure cleanup

Date: 2026-08-26 · Status: **complete** · Owner: Claude
Tracker: [`2026-08-26-docs-structure-cleanup.track.yaml`](2026-08-26-docs-structure-cleanup.track.yaml)

## Objective

Bring this fork's `docs/` up to the full record system: the change-record and
handoff surfaces that are currently missing, an accurate cross-task status
board, and a recording guardrail — without inflating the router files or
touching upstream-owned content.

## Input boundary (policy provenance)

- **Requirement:** the user asked to "clean and update current doc structure"
  and named the organizing skill. That sets the scope.
- **Implementation choices** below (which folders to add, the upstream-safety
  constraint, guardrail wiring) are agent-selected maintainer guardrails, not
  user-authored policy.
- No no-reference boundary applies to this task.

## Controlling constraint: this is a fork

`AGENTS.md` is 1026 lines, but the diff against `upstream/main` is **15 lines** —
the body is upstream's handbook, not this fork's sprawl. `CLAUDE.md` is
8 lines diverged. Rewriting either to "make the router thin" would create a
permanent conflict surface on every upstream sync
([practices/upstream-sync-and-port.md](../../practices/upstream-sync-and-port.md))
while deleting content the fork does not own.

**Initial decision (agent):** treat the fork-owned `## Documentation` block in
each router as the only editable region, leaving upstream's body untouched.

**Superseded — owner decision, 2026-08-26.** The user reviewed that trade-off
and directed the full restructure: *"AGENTS.md and CLAUDE.md should be
restructured as required."* The ongoing merge-conflict cost is accepted.
Executed in phase 2 below. The agent's job here was to price the trade, not to
decide it — see
[the learning record](../../learning/2026-08-26-fork-router-editable-region.md).

## Scope

In scope — the gaps found by survey:

1. `docs/changelogs/` does not exist (+ README index).
2. `docs/HANDOFF.md` does not exist.
3. Docs-recording guardrail not installed (`core.hooksPath` unset, no
   `check-docs-recorded.sh`, no commit hooks).
4. Router `## Documentation` blocks omit changelogs, handoff, and the
   record-keeping reminder.
5. No target-owned technical-lead/dev-worker operations guide.
6. `docs/plans/README.md` is a plain index, not the status board the record
   rules require (no owner, dependency, next action, or last-updated columns),
   and it does not state the plan-record shape for the next agent.
7. `docs/index.md` does not list changelogs or the handoff.

Out of scope: upstream's `AGENTS.md` body, `docs/architecture/` content (source
layout unchanged this session), and any `rust/` code.

## Verification

- `python3 <skill>/scripts/check_target_routers.py .` passes.
- Every new/changed doc link resolves (link sweep over `docs/`).
- `docs/` prose is English-only.
- No private identifiers (API keys, gateway hosts, tokens) in any record.
- Guardrail: `git config core.hooksPath` resolves and the hook files are
  executable.

## Record closure matrix

| Record | Status |
| --- | --- |
| Architecture | Not applicable — this task changed no source layout, entry point, or workflow; `docs/architecture/` still matches the tree. |
| Usage | Done earlier this session — [mcp-multi-agent-setup](../../usage/tools/mcp-multi-agent-setup.md). |
| Issues | Done earlier this session — [binary drift](../../issues/2026-08-26-installed-binary-drift-from-build-tree.md). |
| Changelog | Done — [2026-08-26 rollout + records](../../changelogs/2026-08-26-mcp-rollout-and-docs-records.md). |
| Learning | Done — [fork router editable region](../../learning/2026-08-26-fork-router-editable-region.md). |
| Practices | Done — [operations guide](../../practices/2026-08-26-technical-lead-dev-worker-operations.md), [guardrail](../../practices/2026-08-26-docs-recording-guardrail.md). |
| Plan | This record; tracker marked complete. |
| Routers / indexes | Done — both routers, `docs/index.md`, plans board, practices/learning/changelogs READMEs. |

## Verification results

| Check | Result |
| --- | --- |
| `check_target_routers.py` | **passed** |
| Local links across 55 markdown files + both routers | **0 broken** |
| English-only prose in `docs/` | passed — the one CJK match is a regex character class in backticks, not prose |
| Privacy sweep (keys, bearer tokens, gateway hosts, ephemeral ports) | clean — remaining matches are placeholders (`"…"`, `your-openai-api-key-here`) |
| Guardrail active | `core.hooksPath` → `config/git-hooks`; both hooks executable |

## Phase 2 — full router restructure (owner-directed)

| Router | Before | After | Content moved to |
| --- | --- | --- | --- |
| `AGENTS.md` | 1070 | **100** | [`docs/usage/library/`](../../usage/library/README.md) — quickstart, agent, browser, tools, production, support (985-line `<browser_use_docs>` block) |
| `CLAUDE.md` | 211 | **108** | [Python code style and change workflow](../../practices/2026-08-26-python-code-style-and-change-workflow.md); architecture and command sections replaced with links |

Also in phase 2:

- Contributor "Local Setup" split out to
  [developers/local-setup.md](../../usage/developers/local-setup.md).
- Extracted pages normalized to one H1 each, with inner headings demoted one
  level (code-fence aware, so `#` comments inside examples were left alone).
- Both routers carry a **merge-resolution note** telling the next person how to
  handle the conflicts this now causes: keep the router thin, apply upstream's
  content change to the matching `docs/usage/library/` page.
- Prompt hook installed at the user's request (`UserPromptSubmit` →
  `prompt-reminder.sh`) in the gitignored `.claude/settings.json`.

**Content preservation check:** every content line of the original 985-line
block is present in the extracted pages. The only line not carried over is the
`</browser_use_docs>` wrapper tag itself.

## Outcome

All seven scope items closed, plus the owner-directed phase 2. Routers went from
1281 combined lines to 208, with zero content loss and no broken links.

## Risks

- Over-correcting the routers into an upstream conflict (mitigated by the
  editable-region decision above).
- Installing hooks the user did not ask for: the guardrail's git hooks are
  repo-local and warn-only; the Claude `Stop` hook is already present from the
  prompting skill. Executable hooks are reported, not silently expanded.
