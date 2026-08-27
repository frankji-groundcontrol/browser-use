# Restructuring a router a fork inherits is an owner decision, not a cleanup

Date: 2026-08-26 · Scope: `CLAUDE.md` / `AGENTS.md` in a repo tracking an upstream

## The lesson

A generic docs rule says router files should stay thin and link outward.
`AGENTS.md` here was **1070 lines**, which trips that rule hard — but the file
was not this fork's sprawl. Measure before acting:

```bash
git diff --stat upstream/main -- AGENTS.md CLAUDE.md
```

The answer was **15 lines** in `AGENTS.md` and 8 in `CLAUDE.md`. The bulk was
upstream's embedded handbook, merged in routinely
([upstream-sync-and-port](../practices/upstream-sync-and-port.md)).

That diff does not decide the question — it **prices** it. Restructuring an
inherited file is not free cleanup; it converts a file that merges silently into
one that conflicts on every upstream edit, forever. That is a real, recurring
maintenance cost, so it is the repository owner's call, not an agent's tidying
reflex.

**The rule:** when a generic structural rule collides with inherited content,
measure the divergence, state the ongoing cost, and ask. Do not restructure on
your own initiative, and do not refuse either — surface the trade and let the
owner choose.

## What was decided here

The owner chose the restructure, accepting the conflict cost. On 2026-08-26:

- `AGENTS.md` 1070 → **99 lines**; the embedded `<browser_use_docs>` block moved
  to [`docs/usage/library/`](../usage/library/README.md), split into quickstart,
  agent, browser, tools, production, and support pages.
- `CLAUDE.md` 211 → **104 lines**; its style, CDP, test, and change-workflow
  sections moved to
  [Python code style and change workflow](../practices/2026-08-26-python-code-style-and-change-workflow.md).
- Both routers now carry an explicit note telling the next merger how to resolve
  the conflict: keep the router thin, apply upstream's content change to the
  corresponding `docs/usage/library/` page.

## Evidence

- `wc -l AGENTS.md` → 1070 before, 99 after.
- `git diff upstream/main -- AGENTS.md` → 15 lines before the restructure: a
  single `## Documentation` block plus one upstream-link correction.
- The extracted block was 985 lines, ~92% of the file, loaded into context by
  every agent session that read the router.

## When to apply it again

Any repo with an `upstream` remote, before restructuring, splitting, or
reformatting a file upstream also maintains — routers, CI config, linter config,
top-level READMEs. Measure the diff, price the conflict, then ask.

The inverse also holds: if the diff shows the fork has been steadily *appending*
to an upstream file, that content is exactly what should be extracted into
`docs/`, because it is what generates conflicts today.

Related: [plan](../plans/2026-08-26-docs-structure-cleanup/2026-08-26-docs-structure-cleanup.md),
[changelog](../changelogs/2026-08-26-mcp-rollout-and-docs-records.md).
