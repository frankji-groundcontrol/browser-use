# Docs-recording guardrail

Date: 2026-08-26 · Trigger: any commit in this repo

Memory is not a guardrail. Records get skipped because nothing asks for them at
the moment of committing, so this repo wires the ask into git itself — which
catches every actor, since Claude, Codex, and humans all commit through git.

## What is installed

| Path | Role |
| --- | --- |
| [`scripts/check-docs-recorded.sh`](../../scripts/check-docs-recorded.sh) | The shared check: warn mode, plus a `--checkpoint` gate |
| [`config/git-hooks/pre-commit`](../../config/git-hooks/pre-commit) | **Warns** when source changes stage no changelog entry. Never blocks. |
| [`config/git-hooks/commit-msg`](../../config/git-hooks/commit-msg) | **Blocks** a `[checkpoint]`-tagged commit that does not stage the handoff trio |
| [`docs/HANDOFF.md`](../HANDOFF.md) | The takeover brief the checkpoint gate requires |
| `core.hooksPath` → `config/git-hooks` | Activates the tracked hooks for this clone |
| `.claude/settings.json` `Stop` hook | In-session nudge for Claude (gitignored, local only) |

The handoff trio a `[checkpoint]` commit must stage: a `docs/changelogs/` entry,
a non-README `docs/plans/` path, and `docs/HANDOFF.md`. Task-board and tracker
completeness are review-enforced, not hook-enforced — a hook can see that a file
is staged, not whether its contents are honest.

## Using it

Ordinary commits need nothing special; the pre-commit hook prints a warning and
lets the commit through. Tag a commit `[checkpoint]` when it should be a
resumable handoff point, and stage the trio first.

`core.hooksPath` is per-clone local config, not something a clone inherits. A
fresh clone must re-run the installer or set it by hand:

```bash
git config core.hooksPath config/git-hooks
```

## Verification

```bash
git config core.hooksPath          # -> config/git-hooks
ls -l config/git-hooks/            # both hooks executable
sh scripts/check-docs-recorded.sh  # runs clean
```

## Failure signals

- `core.hooksPath` is unset — the hooks are present but inert. This is the
  normal state of a fresh clone and the most likely reason the guardrail is
  silently doing nothing.
- A `[checkpoint]` commit succeeds without a handoff update — check that the
  hook is executable.
- The handoff is a template with unfilled placeholders. The gate proves a file
  was staged, not that it says anything; that one is on review.

Related: [technical lead and dev worker operations](2026-08-26-technical-lead-dev-worker-operations.md),
[changelogs](../changelogs/README.md), [handoff](../HANDOFF.md).
