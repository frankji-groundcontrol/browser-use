# Phased implementation plan (TDD)

Every phase is test-first: write the failing test, run it to confirm it fails,
implement until green, then refactor. Never mock CDP or the browser — only the
LLM may be faked (mirrors the Python test rule). See
[tdd-strategy.md](tdd-strategy.md).

Each phase below lists **deliverables**, the **tests written first**, and an
**exit criterion**. Ordered so the first installable artifact replaces the
Python MCP server as early as possible.

## Phase 0 — Scaffold & harness

Deliverables:
- Cargo workspace (`crates/*`, `xtask/`), `rust-toolchain.toml` (pin 1.94),
  `.github`/CI running `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`.
- `xtask capture-fixtures` skeleton + a Python fixture-capture script that dumps
  CDP trees, serialized DOM, and schema-optimizer outputs from the live Python
  impl into `crates/*/tests/fixtures/`.
- First **failing** conformance test that boots a headless Chrome and expects a
  (not-yet-existing) `bu-cdp` client to complete `Browser.getVersion`.

Exit: `cargo test` runs, the harness compiles, the first test fails for the right
reason (unimplemented), CI is green on fmt/clippy.

## Phase 1 — Installable MVP: Rust MCP server (14 low-level tools)

The first artifact we can **install and replace** with.

Deliverables:
- `bu-cdp`: connect to a CDP URL, request/response + event routing, per-call
  timeout.
- `bu-session` (minimal): attach to an existing browser, enumerate page targets,
  focus/switch, navigate, single multiplexed socket. (Launching Chromium can come
  from `local-browser` watchdog later; for MVP accept a `--cdp-url` or spawn via a
  thin helper.)
- `bu-dom` (state subset): enough three-tree fusion to build `get_state`'s
  selector map + coordinates.
- `bu-tools` (low-level 14): navigate, click, type, get_state, get_html,
  screenshot, scroll, go_back, list_tabs, switch_tab, close_tab, list_sessions,
  close_session, close_all.
- `bu-mcp` server: stdio JSON-RPC, `initialize` + `tools/list` (14) +
  `tools/call`; logs to stderr only.
- `bu-core`: the `browser-use` binary with a `--mcp` flag.

Tests first:
- Golden: DOM serialization + selector-map for captured fixtures.
- Live: an MCP stdio harness that runs the Rust server against headless Chrome and
  drives `initialize → tools/list → browser_navigate(data:…) → browser_get_state`,
  asserting the button/element appears and `close_all` cleans up.

Exit: the Rust `browser-use --mcp` passes the 14-tool live conformance test; it
can be pointed at by the existing agent wrapper and returns identical tool shapes.
**This is the first replace-the-install milestone.**

## Phase 2 — Perception parity + the extract tool

Deliverables:
- `bu-dom`: full 5-stage serializer, cross-origin iframe recursion, paint-order
  cull, bbox filter, element hashing/MatchLevel.
- `bu-llm` (openai-compatible only, via `async-openai`): `ChatModel::ainvoke` +
  `ainvoke_structured`, `SchemaOptimizer`, JSON-repair.
- `bu-tools`: the `extract`/`browser_extract_content` tool using
  `page_extraction_llm`.

Tests first:
- Golden: serializer byte-parity vs Python fixtures; SchemaOptimizer parity.
- Live: `browser_extract_content` against a local HTML server returns the expected
  facts (LLM faked in unit tests; real gateway in an opt-in integration test).

Exit: 16-tool MCP surface at parity for a representative page corpus.

## Phase 3 — Event bus, watchdogs, autonomous agent

Deliverables:
- `bu-bus`: ask-pattern bus with `event_result` semantics + timeouts.
- Core watchdogs: local-browser (launch/kill Chromium), downloads, security
  (domain policy), dom.
- `bu-agent`: perceive→decide→act step loop, dynamic `AgentOutput`, message
  manager, loop detection; `retry_with_browser_use_agent`.
- `bu-mcp`: honor the beta `agent.run_task`/`agent.event` JSON-RPC contract.

Tests first:
- Bus unit tests for ordering/parenting/timeouts/aggregation.
- Conformance: feed a simple task+config through both the Python beta bridge and
  the Rust `agent.run_task`; diff `history.events`.

Exit: a simple autonomous task completes via the Rust agent and matches the beta
contract; `retry_with_browser_use_agent` works end-to-end.

## Phase 4 — Parity hardening

Additional LLM providers (anthropic/google/…), remaining watchdogs, filesystem
VFS, telemetry, cloud sync — added by demand, each behind its own TDD cycle and
cfg-feature. The `sandbox/` module stays a Python sidecar.

## Definition of done (for the goal)

Phase 1 is the minimum bar to "install and replace the current install"; Phases
2–3 bring the two LLM tools and the autonomous agent to parity. Track live status
in [progress.md](progress.md).
