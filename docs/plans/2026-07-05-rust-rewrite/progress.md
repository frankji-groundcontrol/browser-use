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
| — | 1d | Full parity is now tracked by the staged [parity-plan.md](parity-plan.md) (post-cutover review). | — |
| 2026-07-05 | parity S1 | **Concurrency + robustness — done.** Single-owner actor serializes browser work (within-process race fixed: 8 concurrent get_state → 1 browser, verified); multi-process isolation verified (4 procs); stable-backendNodeId click cache (no TOCTOU); tracing + CDP-spam hygiene; bounded go_back/navigate; resilient tab listing. All live tests green (serial). | done |
| — | parity S2 | Tool-contract parity (click coord+new_tab, type clear+mask, get_state rich shape, isError convention, sessions). | in progress |
| 2026-07-05 | parity S2 | **Tool contracts — done.** click coord+new_tab, type clear+mask, isError convention. (commit `fc252b7`) | done |
| 2026-07-05 | parity S3 | **Full three-tree DOM serializer — done.** DOM+DOMSnapshot+AX fusion; JS-listener/AX/heuristic interactive detection (detects `<div onclick>`/`addEventListener`); visibility filter; scroll-normalized coords. 18 live tests green. (commit `cbaba9c`) | done |
| 2026-07-05 | parity S4 | **bu-llm + `browser_extract_content` → 15/16 — done.** Reqwest OpenAI-compatible client (no wrapper needed); extract verified vs the real gateway. (commit `52f9132`) | done |
| — | parity S5 | Agent loop (`bu-agent`) + `retry_with_browser_use_agent` → 16/16 (capstone). | in progress |
| — | 2 | Extract tool + `bu-llm` (openai-compatible) parity. | pending |
| — | 3 | Event bus + watchdogs + autonomous agent (beta JSON-RPC conformance). | pending |
| — | 4 | Provider/watchdog/parity hardening + cross-platform release. | pending |

## Notes / decisions

- MVP is scoped to the MCP server (the surface the coding agents use) so the first
  build can replace the current install; the full agent loop is Phase 3.
- Prior art: `browser_use/beta/` already drives a native Rust core over JSON-RPC;
  its contract is the conformance oracle, not code we own.
- **Cutover done (per directive).** All 4 agents' `browser-use` MCP server now runs
  the Rust `browser-use-rs --mcp` (14 tools; verified connected in
  claude/codex/hermes/opencode). The Python install (uv-tool binary + wrapper +
  `~/.config/browseruse/config.json`) is kept ON DISK for rollback — nothing was
  deleted.
  - **Known regressions until Phase 1d:** the 2 LLM tools
    (`browser_extract_content`, `retry_with_browser_use_agent`) return "not
    implemented"; element detection is a first-cut selector map (lower quality than
    the Python serializer on complex pages). Closing these = full parity.
  - **Rollback (one edit per agent):** repoint `browser-use` back to the Python
    wrapper `~/.local/share/uv/tools/browser-use/bin/python
    ~/.config/browseruse/mcp-launch.py` with env `OPENAI_API_KEY` + `OPENAI_BASE_URL`.
    e.g. `claude mcp remove browser-use -s user && claude mcp add browser-use -s user
    -e OPENAI_API_KEY='${OPENAI_API_KEY}' -e OPENAI_BASE_URL='${OPENAI_BASE_URL}' --
    <python> <wrapper>`.
- CI lives at `rust/ci/rust.yml` (not `.github/workflows/`): the push token lacks
  `workflow` scope. A maintainer with that scope should move it into
  `.github/workflows/`.
