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
| 2026-07-05 | parity S5 | **Agent loop + `retry_with_browser_use_agent` → 16/16 — done.** `bu-agent` perceive→decide→act loop; verified: live run completed a task in 2 steps with Python's exact report format. (commit `324bfb0`) | done |
| 2026-07-05 | tools parity | **Rust `tools/list` byte-identical to Python: 16 tools, same order, 0 schema diffs.** All 4 agents on `browser-use-rs` + OpenAI env. Concurrency correct (actor + per-process isolation). | done |
| 2026-07-05 | parity S3.4/3.5 | **DOM element-set parity — done.** Ported Python's PaintOrderRemover (drop interactive elements fully covered by higher-painted opaque rects) + `_apply_bounding_box_filtering` (collapse a child ≥99% contained in a propagating interactive parent; form-control/onclick/aria-label carve-outs). 4 live tests (opaque modal, icon+text button, tabbable child, input-in-link). (commit `9168de0`) | **done** |
| 2026-07-05 | refactor | **`bu-cdp/lib.rs` 1765 → 913**, split into `geometry.rs`/`dom.rs`/`discovery.rs` (no monolith). Pure move, all tests green. (commit `4850197`) | **done** |
| 2026-07-05 | agent parity | **Vision + multi-action + reasoning — done.** `bu-llm` modular (multimodal `message.rs` + `LlmProvider` enum); `bu-agent` modular (`AgentOutput` with evaluation/memory/next_goal + ordered actions, screenshot attached when `use_vision`, batch stops after nav/click, extraction fed back). Vision **verified end-to-end** vs the real gateway (agent read the H1 from a screenshot in ~2s). (commit `48f17a6`) | **done** |
| 2026-07-05 | agent parity | **AWS Bedrock provider — done.** `bu-llm` feature `bedrock` (Converse API via aws-sdk-bedrockruntime 1.135; text+PNG blocks; SigV4 via SDK), selected by `MODEL_PROVIDER=bedrock`, forwarded through bu-mcp/bu-core. Compiles + clippy clean + 5 unit tests; not live-tested (no AWS creds, same as Python). (commit `126b7d9`) | **done** |
| 2026-07-05 | **FULL PARITY** | **Both documented gaps closed.** DOM element-set matches Python on occluded/nested pages; agent has vision (verified), multi-action, reasoning fields, and the Bedrock provider. `tools/list` still byte-identical. Default binary rebuilt + reinstalled (16 tools). | **done** |
| — | 4 | Remaining hardening: `allowed_domains`/SecurityWatchdog enforcement (schema advertised, not yet enforced), cross-platform release. | pending |

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
  - **All Phase-1d regressions closed (full parity reached):** both LLM tools work;
    the selector map now ports Python's paint-order + bbox filtering; the agent has
    vision + multi-action + reasoning; Bedrock is available behind a feature.
  - **Deployment requirement:** `OPENAI_BASE_URL` must include the API path
    (`.../v1`) — `bu-llm` posts `{base_url}/chat/completions` (same as Python's
    OpenAI client). The 4 agents' MCP config is correct (`https://…/v1`); a bare host
    makes the LLM tools hit the gateway's HTML landing page → "failed to parse".
  - **Rollback (one edit per agent):** repoint `browser-use` back to the Python
    wrapper `~/.local/share/uv/tools/browser-use/bin/python
    ~/.config/browseruse/mcp-launch.py` with env `OPENAI_API_KEY` + `OPENAI_BASE_URL`.
    e.g. `claude mcp remove browser-use -s user && claude mcp add browser-use -s user
    -e OPENAI_API_KEY='${OPENAI_API_KEY}' -e OPENAI_BASE_URL='${OPENAI_BASE_URL}' --
    <python> <wrapper>`.
- CI lives at `rust/ci/rust.yml` (not `.github/workflows/`): the push token lacks
  `workflow` scope. A maintainer with that scope should move it into
  `.github/workflows/`.
