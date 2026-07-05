# Progress log — Rust rewrite

Update after every green milestone. Newest first.

| Date | Phase | Milestone | Status |
| --- | --- | --- | --- |
| 2026-07-05 | — | Plan authored (index/architecture/porting-map/plan/tdd) from the codebase deep-read. | done |
| 2026-07-05 | — | Plan reviewed (plan-eng-review + outside voice); 13 findings folded; see [review.md](review.md). | done |
| 2026-07-05 | 0 | `franky-rust` branch + workspace scaffold + CI + first failing test. | pending |
| — | 1 | Rust MCP server, 14 low-level tools, live-Chrome conformance (installable MVP). | pending |
| — | 2 | Full DOM serializer + extract tool parity. | pending |
| — | 3 | Event bus + watchdogs + autonomous agent (beta JSON-RPC conformance). | pending |
| — | 4 | Provider/watchdog/parity hardening. | pending |

## Notes / decisions

- MVP is scoped to the MCP server (the surface the coding agents use) so the first
  build can replace the current install; the full agent loop is Phase 3.
- Prior art: `browser_use/beta/` already drives a native Rust core over JSON-RPC;
  its contract is the conformance oracle, not code we own.
