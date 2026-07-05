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
- **Spike first (highest-risk-first): evaluate existing Rust CDP crates**
  (`chromiumoxide`, `rust-headless-chrome`) as the transport + session + actor
  foundation before hand-rolling `bu-cdp`/`bu-session`/`bu-actor`. If one cleanly
  supports the multiplexed `setAutoAttach(flatten)` session model + reconnect,
  build on it (Layer 1, boring-by-default) and those three crates shrink to thin
  wrappers — cutting the highest-risk work. Timebox ~1 day; record the decision in
  [progress.md](progress.md). This is the single biggest architecture call.
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

The first artifact we can **install and replace** with. **Scope reality (per eng
review): this is NOT a thin shim.** The 14 tools bottom out in a correct DOM
serializer plus a real Chromium launch, so Phase 1 is a substantial chunk of the
stack. Two deliberate simplifications keep it tractable and resolve two review
findings:

- **Drive the tools through `bu-actor` (direct CDP), NOT the event bus.**
  `bu-actor` is the bus-free imperative facade (as in Python). The 14 low-level
  tools map to direct CDP calls; the bubus event bus + watchdogs are deferred to
  Phase 3, where the autonomous agent actually needs them. This resolves the "bus
  deferred but the P1 tools are bus-backed" contradiction: in the Rust MVP the
  tools are **actor-backed**, and `architecture.md`'s "tool = bus.dispatch()" is a
  Phase-3 shape, not P1.
- **Build `bu-cdp` on `chromiumoxide`** (transport + generated CDP types); keep our
  own single-lock session map on top (chromiumoxide's Handler model does not match
  the `Mutex<SessionState>` invariant).

Deliverables:
- `bu-cdp`: chromiumoxide-based transport + CDP types; request/response + event
  routing; per-call timeout.
- `bu-session` (minimal): own session map; attach/enumerate page targets;
  focus/switch; navigate; single multiplexed socket. **Includes Chromium launch**
  (spawn local Chromium with the Python launch args, incl. `--no-sandbox` on
  Ubuntu; a `--cdp-url` override for attach-mode). A bare Chromium launch is
  REQUIRED for drop-in — a server that only accepts a pre-supplied CDP endpoint is
  not a drop-in. Default extensions (uBlock/ICDC/ClearURLs) are deferred (see
  NOT-in-scope).
- `bu-dom` (**FULL serializer — irreducible**): the complete 5-stage pipeline
  (simplify → paint-order cull → optimize → bbox filter → index assignment). A
  subset is NOT viable — `selector_map` index == `backendNodeId` is all-or-nothing;
  a different index makes the agent click the wrong element.
- `bu-actor`: the imperative CDP facade the 14 tools call directly.
- `bu-tools` (low-level 14) over `bu-actor`: navigate, click, type, get_state,
  get_html, screenshot, scroll, go_back, list_tabs, switch_tab, close_tab,
  list_sessions, close_session, close_all.
- `bu-mcp` server: stdio JSON-RPC; `initialize` advertises the SAME
  `protocolVersion` + `serverInfo{name:'browser-use'}` + tools capability as the
  Python `mcp==1.26.0` server; `tools/list` returns the 14 with **byte-identical
  input schemas**; `tools/call`; logs to stderr only (stdout stays pure JSON-RPC).
- `bu-core`: the `browser-use` binary with `--mcp`.

Bug-for-bug decisions (enumerate keep-vs-fix; default **keep** for a true drop-in):
`browser_close` stays absent from `tools/list` (dead in Python — keep); tool
descriptions/defaults (e.g. `new_tab` default false) copied verbatim;
`get_state`/`screenshot` return the PNG as a separate `ImageContent` item (keep).

Tests first (see [tdd-strategy.md](tdd-strategy.md)):
- Golden: `tools/list` + `initialize` diffed against the **installed Python
  `browser-use --mcp` server** (verbatim JSON) — the real drop-in oracle, and one
  that is actually runnable in this repo (unlike the unowned vendor binary).
- Golden: DOM serialization + selector map — **field-wise with tolerance against a
  pinned Chromium build**, not byte-equality (layout coords vary by build/viewport/
  scale/headless-font-rendering).
- Live: an MCP stdio harness runs the Rust server against a **self-launched**
  headless Chromium and drives `initialize → tools/list → navigate(data:…) →
  get_state`, asserts the element appears, `close_all` cleans up.

Exit: the Rust `browser-use --mcp` self-launches Chromium and passes the live
14-tool conformance + the `tools/list`/`initialize` golden vs the Python server. It
drops into the 4-agent wiring by changing only the command path.
**First replace-the-install milestone.**

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
- **Reuse option:** `retry_with_browser_use_agent` MAY shell out to the existing
  `browser-use-terminal` binary over its JSON-RPC contract instead of a native
  agent loop, if the native loop isn't ready (reuse over rebuild — ships the tool
  sooner; the native loop can replace it later).
- `bu-mcp`: honor the beta `agent.run_task`/`agent.event` JSON-RPC contract.

Tests first:
- Bus unit tests for ordering/parenting/timeouts/aggregation.
- Conformance: the Rust `agent.run_task` emits the documented beta JSON-RPC shapes
  (schema-level assertions). NOTE: an end-to-end diff against the vendor
  `browser-use-terminal` binary is only possible if that binary is installed — it
  is **not owned/vendored here**, so treat the wire-contract doc as the spec and
  the installed **Python autonomous agent** as the behavioral oracle for simple
  tasks.

Exit: a simple autonomous task completes via the Rust agent;
`retry_with_browser_use_agent` works end-to-end (native, or via the beta-binary
reuse option if that binary is present).

## Phase 4 — Parity hardening

Additional LLM providers (anthropic/google/…), remaining watchdogs, filesystem
VFS, telemetry, cloud sync — added by demand, each behind its own TDD cycle and
cfg-feature. The `sandbox/` module stays a Python sidecar.

## Build & distribution (make it installable)

The goal is to *replace the installed* `browser-use --mcp`. Phase 1's exit is not
just green tests — it's an installed binary the four agents can spawn:

- `cargo build --release -p bu-core` → a `browser-use` binary (entry `--mcp`).
- Install to `~/.local/bin/` under a distinct name first (e.g. `browser-use-rs`)
  for A/B against the Python `uv tool` binary; re-point the MCP launcher / agent
  configs at it. The 4-agent wiring in
  [docs/usage/tools/mcp-multi-agent-setup.md](../../usage/tools/mcp-multi-agent-setup.md)
  is unchanged except the command path.
- **Reversibility:** keep the Python install available for rollback until the Rust
  server passes the live conformance suite; only then shadow the `browser-use`
  name.
- Cross-platform release (`cargo-dist` / GitHub Releases; linux+darwin, amd64+arm64)
  is Phase 4, not MVP — see NOT-in-scope.

## Definition of done (for the goal)

Phase 1 is the minimum bar to "install and replace the current install"; Phases
2–3 bring the two LLM tools and the autonomous agent to parity. Track live status
in [progress.md](progress.md).
