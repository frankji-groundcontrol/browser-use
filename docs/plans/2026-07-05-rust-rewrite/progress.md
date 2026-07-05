# Progress log — Rust rewrite

Update after every green milestone. Newest first.

| Date | Phase | Milestone | Status |
| --- | --- | --- | --- |
| 2026-07-05 | — | Plan authored (index/architecture/porting-map/plan/tdd) from the codebase deep-read. | done |
| 2026-07-05 | — | Plan reviewed (plan-eng-review + outside voice); 13 findings folded; see [review.md](review.md). | done |
| 2026-07-05 | 0 | `franky-rust` + 11-crate workspace + CI + first RED test. Compiles; RED fails as intended. (commits `fe2082`/`e0fefd`) | **done** |
| 2026-07-05 | 1a | `bu-dom::serialize_dom` first slice GREEN; `bu-mcp` rmcp server — `initialize` (serverInfo `name=browser-use`) + `tools/list` (14 tools, schemas from Python) verified over stdio; `browser-use-rs --mcp` built + installed to `~/.local/bin` + registered A/B in claude (✔ Connected). `tools/call` still stubbed. (commit `7a8af4`) | **done** |
| — | 1b | Implement the 14 `tools/call` bodies: `bu-cdp` live connect + `bu-session` Chromium launch + `bu-actor` + full `bu-dom`. Golden `tools/list`/`initialize` vs Python + live-Chrome conformance. **Replace Python only after this passes.** | pending |
| — | 2 | Extract tool + `bu-llm` (openai-compatible) parity. | pending |
| — | 3 | Event bus + watchdogs + autonomous agent (beta JSON-RPC conformance). | pending |
| — | 4 | Provider/watchdog/parity hardening + cross-platform release. | pending |

## Notes / decisions

- MVP is scoped to the MCP server (the surface the coding agents use) so the first
  build can replace the current install; the full agent loop is Phase 3.
- Prior art: `browser_use/beta/` already drives a native Rust core over JSON-RPC;
  its contract is the conformance oracle, not code we own.
- **Install is A/B, not a replacement (deliberate).** `browser-use-rs` is installed
  and registered beside the Python `browser-use`; the Python server stays the
  functional primary for all 4 agents. Shadowing the `browser-use` name waits until
  Phase 1b makes the 14 `tools/call` bodies real and live conformance passes —
  replacing a stubbed server would break the agents. Reversibility over speed.
- CI lives at `rust/ci/rust.yml` (not `.github/workflows/`): the push token lacks
  `workflow` scope. A maintainer with that scope should move it into
  `.github/workflows/`.
