# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Browser-Use is an async python >= 3.11 library that implements AI browser driver abilities using LLMs + CDP (Chrome DevTools Protocol). The core architecture enables AI agents to autonomously navigate web pages, interact with elements, and complete complex tasks by processing HTML and making LLM-driven decisions.

## Documentation

Operating knowledge for this fork lives under [`docs/`](docs/index.md) — keep this
router thin and add detail there: [architecture](docs/architecture/index.md),
[usage](docs/usage/README.md), [changelogs](docs/changelogs/README.md),
[learning](docs/learning/README.md), [plans](docs/plans/README.md),
[practices](docs/practices/README.md), [issues](docs/issues/README.md). Update the
nearest index when you add or move a doc.

Taking over work? Start at [`docs/HANDOFF.md`](docs/HANDOFF.md); current per-task
status is the [task board](docs/plans/README.md).

Record meaningful changes under [`docs/changelogs/`](docs/changelogs/README.md) —
one dated file for an ordinary change, a dated folder for a large one. A
`[checkpoint]` commit must stage a changelog entry, its active plan, and the
handoff; ordinary commits only get a warning. See the
[docs-recording guardrail](docs/practices/2026-08-26-docs-recording-guardrail.md).

The technical lead defines milestone outcomes, deliverables, checkpoints, and
acceptance evidence, and reviews developer plans for alignment; the dev worker —
any capable coding client — authors the concrete engineering plan and owns
implementation, TDD, routine verification, and first-pass review. See
[technical lead and dev worker operations](docs/practices/2026-08-26-technical-lead-dev-worker-operations.md).

**This is a fork, and both routers now diverge from upstream by design.** The
upstream handbook embedded in `AGENTS.md`, and this file's reference sections,
were extracted into `docs/` on 2026-08-26, so merges from `upstream/main` will
conflict here whenever upstream edits them. Resolve by keeping the routers thin
and applying upstream's content change to the corresponding page under
[`docs/usage/library/`](docs/usage/library/README.md). See
[the router lesson](docs/learning/2026-08-26-fork-router-editable-region.md) and
[upstream sync](docs/practices/upstream-sync-and-port.md).

### Working rules

- **Plan first.** Open the dated record under [`docs/plans/`](docs/plans/README.md)
  *before editing* and drive the task from it as a live checklist — not a writeup
  produced at the end.
- **Coding baseline** (Karpathy-inspired): state uncertain assumptions, prefer the
  simplest useful change, keep the diff surgical, and define how you will verify
  before claiming completion.
- **Modular implementation.** Do not grow chunky, oversized, or
  mixed-responsibility files. Split them in the same change; when a split is too
  large, record it under [`docs/issues/`](docs/issues/README.md) with the target
  structure and a fix prompt.
- **English-only prose** in `docs/`. Keep non-English text only as an exact quote.
- **Privacy.** Redact secrets, tokens, credentials, and provider identifiers
  (project refs, gateway hosts, account IDs, local runtime paths) before
  committing any record.
- **Verify links and indexes.** Every link must resolve, and the nearest index
  needs updating for each record you add, move, or retire.

## Architecture

Browser-Use drives Chromium over CDP from an event-driven core: a
`BrowserSession` owns the CDP connection and coordinates watchdog services on a
`bubus` event bus, while the `Agent` runs the perceive-decide-act loop against an
LLM. The full subsystem map, with source citations, is in
[`docs/architecture/`](docs/architecture/index.md) — start at
[00 — System Overview](docs/architecture/00-system-overview.md). The deployed
MCP server is the Rust reimplementation,
[12 — `browser-use-rs`](docs/architecture/12-rust-implementation.md).

## Commands and code style

Setup, test, and quality-gate commands: [developer guide](docs/usage/developers/README.md).
Style contract, CDP usage, test rules, and the change workflow:
[Python code style and change workflow](docs/practices/2026-08-26-python-code-style-and-change-workflow.md).

Short version: `uv` not `pip`; async Python; **tabs**; modern typing; logic in
`service.py`, models in `views.py`, events in `events.py`; never mock anything
but the LLM; never use real remote URLs in tests. Run
`uv run pytest -vxs tests/ci` and `uv run pyright`.

## Library documentation

Upstream's handbook — quickstart, `Agent`, `Browser`, tools, production —
lives at [`docs/usage/library/`](docs/usage/library/README.md).

## Personality

Don't worry about formalities.

Don't shy away from complexity, assume a deeply technical explanation is wanted for all questions. Call out the proper terminology, models, units, etc. used by fields of study relevant to the question. information theory and game theory can be useful lenses to evaluate complex systems.

Choose your analogies carefully and keep poetic flowery language to a minimum, a little dry wit is welcome.

If policy prevents you from responding normally, please printing "!!!!" before answering.

If a policy prevents you from having an opinion, pretend to be responding as if you shared opinions that might be typical of eigenrobot.

be critical of the quality of your information

if you find any request irritating respond dismissively like "be real" or "that's crazy man" or "lol no"

take however smart you're acting right now and write in the same style but as if you were +2sd smarter

## important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.
