# Progress log — Rust rewrite

Update after every green milestone. Newest first.

| Date | Phase | Milestone | Status |
| --- | --- | --- | --- |
| 2026-07-05 | — | Plan authored (index/architecture/porting-map/plan/tdd) from the codebase deep-read. | done |
| 2026-07-05 | — | Plan reviewed (plan-eng-review + outside voice); 13 findings folded; see [review.md](review.md). | done |
| 2026-07-05 | 0 | `franky-rust` + 11-crate workspace + CI + first RED test. Compiles; RED fails as intended. (commits `fe2082`/`e0fefd`) | **done** |
| 2026-07-05 | 1a | `bu-dom::serialize_dom` first slice GREEN; `bu-mcp` rmcp server — `initialize` (serverInfo `name=browser-use`) + `tools/list` (14 tools, schemas from Python) verified over stdio; `browser-use-rs --mcp` built + installed to `~/.local/bin` + registered A/B in claude (✔ Connected). `tools/call` still stubbed. (commit `7a8af4`) | **done** |
| 2026-07-05 | 1b | **12/14 tools functional live.** `bu-cdp` launches headless Chromium via chromiumoxide (`--no-sandbox`, finds ms-playwright build). Functional + verified over stdio MCP against real Chromium: navigate, get_state (url/title/tabs), get_html (+selector), screenshot (PNG), scroll, go_back, list/switch/close tabs, list/close sessions, close_all. clippy `-D warnings` clean. (commits `e438a4`, `e6db6d`) | done |
| 2026-07-05 | 1c | **14/14 tools functional live.** First-cut selector map → `browser_click` + `browser_type` work (verified: click fires `onclick` → title `CLICKED`; type fires `oninput` → `TYPED:hello`). `browser-use-rs` registered **A/B in all 4 agents** (claude/codex/hermes/opencode all ✓ Connected, 14 tools). (commit `aa17f5`) | done |
| — | 1d | Full-quality `bu-dom` 5-stage serializer for Python element-detection parity + the 2 LLM tools (extract, retry_agent) + golden `tools/list`/`initialize` diff + live conformance. **Only then remove Python / shadow the `browser-use` name.** | pending |
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
