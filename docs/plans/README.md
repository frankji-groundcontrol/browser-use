# Plan records

Dated, living task plans. A plan is opened at the **start** of a multi-step task
and kept current as work proceeds — it is the checklist that keeps the task from
losing an aspect, not a writeup produced at the end.

## Task board

The authoritative cross-task status view. Several tasks may be `in progress` at
once, but each needs a real owner and a current next action. Update the row and
the detailed plan together whenever work starts, blocks, changes owner, or
completes; keep checklists, decisions, and evidence in the plan rather than
growing this table.

| Task | Status | Owner | Depends on / blocker | Next action | Updated | Plan |
| --- | --- | --- | --- | --- | --- | --- |
| Docs structure cleanup — add the missing record surfaces and guardrail | complete | Claude | — | Shipped as `405ba249c`; live on both hosts | 2026-08-27 | [plan](2026-08-26-docs-structure-cleanup/2026-08-26-docs-structure-cleanup.md) |
| Unify the binary deploy layout across both hosts | pending | Unassigned | Needs a call: symlink vs file copy | Pick one, then record it in the deploy practice | 2026-08-27 | [issue](../issues/2026-08-26-installed-binary-drift-from-build-tree.md) |
| Rust rewrite — reimplement browser-use in Rust with TDD | in progress | Unassigned | — | Resume at the next step named in the plan | 2026-07-05 | [plan](2026-07-05-rust-rewrite/index.md) |
| Franky repo setup — repoint the fork, document install, organize docs | in progress | Unassigned | — | Confirm remaining setup items or close the plan | 2026-07-05 | [plan](2026-07-05-franky-repo-setup/index.md) |

## Plan registry

One entry per detailed plan, so historical plans stay discoverable.

- [2026-07-05 — Franky repo setup](2026-07-05-franky-repo-setup/index.md) —
  repoint to the fork, document the local install, organize repo docs.
- [2026-07-05 — Rust rewrite](2026-07-05-rust-rewrite/index.md) — modular plan
  to re-implement browser-use in Rust with TDD on the `franky-rust` branch.
- [2026-08-26 — Docs structure cleanup](2026-08-26-docs-structure-cleanup/2026-08-26-docs-structure-cleanup.md)
  — add the changelog, handoff, and recording-guardrail surfaces this fork was
  missing, without disturbing upstream-owned files.

## Which shape to use

Pick by whether the plan carries a tracker, because that decides the path:

| Situation | Path |
| --- | --- |
| Small plan, no tracker | `docs/plans/YYYY-MM-DD-title.md` |
| Phases, logs, or decisions, no tracker | `docs/plans/YYYY-MM-DD-title/index.md` plus child files |
| **Carries a tracker** | `docs/plans/YYYY-MM-DD-title/` with **every file repeating the slug**: `YYYY-MM-DD-title.md`, `YYYY-MM-DD-title.track.yaml`, `YYYY-MM-DD-title.track.history.jsonl` |

Inside a tracked folder there is never a bare `index.md` or `track.yaml`, and
never a tracker hidden elsewhere in the repo. The slug prefix is what keeps
files distinguishable in fuzzy-open and editor tabs once many plans exist, and
it stops a plan and its tracker drifting apart under a rename.

Promoting a flat plan to folder form is `mkdir` + `mv` — the **filename never
changes**, but the path gains a level, so re-point this README's rows, changelog
links, and the plan's own relative links (`../architecture/` becomes
`../../architecture/`). Promoting an `index.md` folder plan to the tracked form
renames `index.md` to `YYYY-MM-DD-title.md`; fix inbound links in the same
change, because the tracker script validates its own path but not a stale entry
file.

## Division of labour

The technical lead defines milestone outcomes and acceptance evidence; the dev
worker — any capable coding client — authors the concrete engineering plan and
owns implementation. See
[technical lead and dev worker operations](../practices/2026-08-26-technical-lead-dev-worker-operations.md).
