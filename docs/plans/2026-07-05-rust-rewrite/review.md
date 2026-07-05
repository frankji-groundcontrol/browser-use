# Eng review — Rust rewrite plan (2026-07-05)

Reviewed with the `plan-eng-review` lens (architecture / code quality / tests /
performance) plus an independent **outside-voice** pass (fresh-context agent;
codex was the intended reviewer but its gateway auth was broken at review time —
now fixed). Run autonomously; every finding was decided here (no unresolved
questions were left for the user).

## Findings & decisions

| # | Sev | Conf | Finding | Decision |
| --- | --- | --- | --- | --- |
| 1 | P1 | 8 | **No thin MCP-only slice exists.** The 14 tools bottom out in the full DOM serializer + Chromium launch (+ bus, if bus-backed). "Phase 1 = thin server" understated the work by ~10x. | **Folded.** Phase 1 reframed as substantial; DOM serializer is irreducible; Chromium launch is in-scope. |
| 2 | P1 | 8 | **DOM serializer can't be split P1/P2.** `selector_map` index == `backendNodeId` is all-or-nothing; a subset yields wrong indices → agent clicks the wrong element. | **Folded.** Full 5-stage serializer moved entirely into Phase 1. |
| 3 | P1 | 8 | **Bus contradiction.** Plan defers the bus to P3 but 12/14 P1 tools are bus-backed in Python. | **Folded.** MVP drives the 14 tools through the **bus-free `bu-actor`** path (direct CDP). Bus + watchdogs are Phase 3 (agent loop only). |
| 4 | P1 | 8 | **Chromium launch punted → not a drop-in.** Python `--mcp` self-launches Chromium (+extensions). | **Folded.** `bu-session` includes Chromium launch in Phase 1; default extensions deferred (NOT-in-scope). |
| 5 | P2 | 7 | **chromiumoxide is a real win the plan hand-waved** ("reuse types OR roll our own"). | **Folded.** Committed to chromiumoxide for transport + CDP types; struck the hand-roll option; keep our own session map. |
| 6 | P2 | 7 | **Conformance oracle unowned.** The beta `browser-use-terminal` binary (the agent-loop oracle) is not installed/owned. | **Folded.** Runnable oracle is the **installed Python `--mcp` server** (tool/protocol drop-in); beta contract is spec-level only. |
| 7 | P2 | 7 | **Tool-schema / `initialize` byte-compat claimed but untested.** | **Folded.** Added a golden test diffing Rust `tools/list` + `initialize` against the Python server verbatim. |
| 8 | P2 | 7 | **Byte-equal DOM goldens will be flaky** (coords vary by Chromium build). | **Folded.** Field-wise-with-tolerance on coords, exact on tree/indices, pinned Chromium build. |
| 9 | P3 | 6 | **Bug-for-bug decisions unlisted** (`browser_close` dead, `gpt-o4-mini` typo, ImageContent split). | **Folded.** Keep-vs-fix defaults enumerated in Phase 1 (default: keep for drop-in). |
| 10 | P3 | 6 | **MCP `initialize` handshake** (protocolVersion/serverInfo) unaddressed. | **Folded** into finding 7's golden. |
| 11 | P3 | 6 | **Distribution absent** — plan's goal is "replace the install" with no build/install path. | **Folded.** Added a Build & distribution section (A/B install, reversibility, cargo-dist deferred). |
| 12 | P3 | 5 | **Concrete first-tests missing** for the codex TDD hand-off. | **Folded.** Six ordered RED tests added to `tdd-strategy.md`. |
| 13 | P3 | 5 | Branch drift: plan says `franky-rust`, which doesn't exist yet. | **Accepted as intentional** — `franky-rust` is the target branch, created at Phase 0/7. |

## Strategic verdict (outside voice: "wrong-approach")

The outside voice argued a full greenfield rewrite is strategically dubious versus
shipping/wrapping the existing native `browser-use-terminal` binary. **Weighed and
overridden by the directed goal** (the user asked for a Rust rewrite on
`franky-rust`). The risk is acknowledged in [index.md](index.md) and mitigated: build
on `chromiumoxide` + `rmcp`, DOM serializer as the one irreducible core, installable
MCP server first. If the vendor binary becomes ownable, wrapping it as MCP is the
cheaper path and should be reconsidered.

## What already exists (reuse, don't rebuild)

- **`browser-use-terminal`** (native Rust agent core) — conformance spec; possible
  Phase-3 backend for `retry_with_browser_use_agent` via its JSON-RPC contract.
- **The installed Python `browser-use --mcp` server** — the runnable drop-in oracle.
- **`chromiumoxide` / `rmcp`** — transport + MCP, instead of hand-rolling.
- **The Python impl** — the golden-fixture source for DOM + SchemaOptimizer.

## NOT in scope (v1)

Per-provider parity beyond OpenAI-compatible; every watchdog; cloud sync; telemetry;
the `sandbox/` cloudpickle executor (Python sidecar); default browser extensions in
the MVP; cross-platform release packaging (Phase 4).

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | issues_folded | 13 findings, all folded or accepted |
| Outside Voice | fresh-context agent | Independent 2nd opinion | 1 | issues_found | verdict wrong-approach; scope hardened, rewrite proceeds per directive |

**CROSS-MODEL:** the two passes agreed on substance; the outside voice was harsher on
scope realism (findings 1–4) and on the strategic bet. Both folded.
**VERDICT:** ENG review complete — plan hardened and cleared to implement on `franky-rust`.

NO UNRESOLVED DECISIONS
