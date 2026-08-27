# Technical lead and dev worker operations

Date: 2026-08-26 · Trigger: any systematic, multi-step change in this repo

Two roles, deliberately separated so planning authority and implementation
authority do not collapse into one another. A "dev worker" here is any capable
coding client — Claude Code, Codex CLI, Qoder, Kimi Code, Grok, or a human. No
specific client is required.

## Who owns what

| | Technical lead | Dev worker |
| --- | --- | --- |
| Owns | Milestone outcomes, deliverables, sequencing, constraints, checkpoints, acceptance evidence | The concrete engineering plan, implementation, TDD, routine verification, first-pass review |
| Writes | The milestone brief | The plan under [`docs/plans/`](../plans/README.md): affected files, approach, dependencies, tests, parallelism, verification commands, risks, record updates |
| Does not | Prescribe the low-level technical path, or hand-execute | Redefine the milestone or its acceptance bar |

The lead reviews the developer-authored plan **once** — for goal alignment,
obvious omissions, scope, and material risk. After that it intervenes only on a
concrete blocker, a material scope or goal conflict, elevated risk, or a failed
acceptance gate. It does not spot-check every step; routine verification is the
dev worker's job.

The lead does **no hands-on execution and no ground-level planning**. If it finds
itself listing files to edit or choosing the technical approach, the roles have
collapsed and the dev worker has lost ownership of the plan it is accountable for.

## Sequence

1. **Lead** writes the milestone brief: outcomes and the evidence that proves
   them. No file lists, no implementation path.
2. **Dev worker** opens the dated plan record and authors the engineering plan
   against it.
3. **Lead** reviews that plan once for alignment. Run or emulate an engineering
   review here when the change is substantial.
4. **Dev worker** implements test-first: failing test, see it fail for the
   expected reason, smallest passing change, then refactor.
5. **Dev worker** keeps the plan and its tracker current *as work proceeds* —
   the plan is the live checklist, not an end-of-task writeup.
6. **Lead** accepts against the evidence defined in step 1.

## Parallelize independent units

When work decomposes into genuinely independent units — separate subsystems to
read or transform, distinct review lenses, per-item verification — run them
concurrently rather than serially. Give any units that *mutate files* their own
git worktree so they cannot conflict. The coordinating agent still owns
integration and final verification.

Only parallelize when the units share no mutable state and cannot conflict on
write. Tightly coupled or trivial work costs more to coordinate than it saves.

## Verification

The milestone is met when the lead's stated acceptance evidence exists — not
when the code merely runs. Record in the plan whether an engineering review ran
and whether TDD was followed, including a reason when it was not applicable.

## Failure signals

- The lead is writing file lists or choosing the implementation path.
- The plan record was written after the work instead of driving it.
- Acceptance rests on "it works" rather than the evidence named up front.
- Parallel units are editing the same files without worktree isolation.

Related: [docs-recording guardrail](2026-08-26-docs-recording-guardrail.md),
[plans board](../plans/README.md).
