# Target Rust architecture

A single Cargo **workspace**, one crate per layer, with dependency edges pointing
strictly downward (transport at the bottom, agent/MCP at the top). This mirrors
the 8-layer Python stack in [docs/architecture/00-system-overview.md](../../architecture/00-system-overview.md).

## Workspace layout

```
franky-rust/                     (workspace root; lives on branch franky-rust)
├── Cargo.toml                   # [workspace] members + shared deps
├── crates/
│   ├── bu-cdp/                  # CDP transport over one multiplexed WebSocket
│   ├── bu-session/             # SessionManager + BrowserSession state machine
│   ├── bu-bus/                 # ask-pattern event bus (bubus analog)
│   ├── bu-dom/                 # three-tree fusion + serializer (pure, sync)
│   ├── bu-tools/               # action registry + ActionResult + extract
│   ├── bu-llm/                 # ChatModel trait + providers + SchemaOptimizer
│   ├── bu-agent/               # perceive→decide→act loop
│   ├── bu-mcp/                 # MCP server + client (rmcp)
│   ├── bu-actor/               # imperative CDP facade (bus-free)
│   ├── bu-config/              # env/config + logging bootstrap
│   └── bu-core/                # re-exports; builds the `browser-use` binary
└── xtask/                      # fixture capture, conformance harness
```

## Per-crate design

### `bu-cdp` — transport
- `tokio` + `tokio-tungstenite`; one reader task parses frames.
- Requests: `HashMap<u64, oneshot::Sender<Value>>` keyed by JSON-RPC id; per-call
  `tokio::time::timeout` (the `TimeoutWrappedCDPClient` 60s analog).
- Events: dispatch by `method` (+ `sessionId`) to a `broadcast`/`mpsc` sink.
- `WebSocketConfig` max_frame_size ≈ 200 MB; `bytes` for payloads.
- CDP types: either reuse `chromiumoxide`'s generated protocol types or
  serde-derive our own from the DevTools PDL (stay faithful to `cdp-use`).

### `bu-session` — the stateful core
- Four maps (targets, sessions, target→sessions, session→target) behind **one**
  `Arc<Mutex<SessionState>>`. A single lock preserves the atomic-update invariant;
  `DashMap` would break it. See
  [01-cdp-transport-and-session-manager.md](../../architecture/01-cdp-transport-and-session-manager.md).
- `enum ConnState` / `enum RecoveryState` guarded by `Notify`/`watch` replace the
  Python reconnect/focus-recovery state machines.
- Per-session lifecycle events → a bounded channel (not the 50 ms GIL poll loop).

### `bu-bus` — event bus
- The make-or-break redesign. mpsc queue + per-dispatch `Vec<oneshot>` reply
  aggregation implementing bubus' `event_result` semantics (ordered FIFO, causal
  parenting, per-event timeout). Consider `ractor`/`kameo`, but a purpose-built
  bus is likely cleaner. Events as `enum Event` with per-variant reply channels;
  handler registration via a `#[watchdog]` proc-macro or an explicit registry
  (no runtime reflection).

### `bu-dom` — perception (pure, highest-confidence port)
- Arena tree: `slotmap`/`indextree` (`Vec<Node>` + `NodeId`), **not**
  `Rc<RefCell>`. `#[serde(skip)]` the parent index.
- Sync serializer pipeline; `sha2` element hashing.
- CONTRACT to preserve exactly: `selector_map` "index" == CDP `backendNodeId`.
- A `FrameResolver` trait decouples it from `bu-session`.

### `bu-tools`
- `#[async_trait] trait Action { type Params; async fn execute(&self, ctx: &ActionCtx) -> ActionResult }`
  plus an object-safe `ErasedAction` in `HashMap<String, Box<dyn ErasedAction>>`.
- `ActionCtx` struct replaces Python's reflection-based special-param DI.
- Dynamic tool schema = programmatic `serde_json::Value` + `jsonschema` validation
  (no compile-time param typing — inherent to dynamic tool discovery).

### `bu-llm`
- `#[async_trait] trait ChatModel` with split `ainvoke` / `ainvoke_structured<T>`.
- `async-openai` (base_url) collapses the whole OpenAI-compatible family; hand-roll
  Anthropic + Gemini over `reqwest` + `serde`. `SchemaOptimizer` is pure
  `serde_json::Value` tree-walking. Keep the JSON-repair heuristics (serde is
  stricter than Python's `json`).

### `bu-agent`, `bu-mcp`, `bu-actor`, satellites
- `bu-agent`: the step loop; dynamic `AgentOutput` becomes `Value`-schema based.
- `bu-mcp`: `rmcp` for both server and client; the server re-exposes `bu-tools`.
- `bu-actor`: clean, bus-free imperative facade; keycode tables via `phf`.
- `bu-config`/`bu-telemetry`/`bu-filesystem`: explicit accessors replace the
  Python metaprogramming (`__getattr__`, PEP-562 lazy imports, monkeypatches).

## Cross-cutting

- Async runtime: `tokio` (multi-thread). Errors: `thiserror` per crate +
  `anyhow` at the binary. Logging: `tracing` (route to **stderr** to keep MCP
  stdout a clean JSON-RPC channel — the Python `stdout-purity` requirement).
- Token accounting: a `TokenTracked<M: ChatModel>` wrapper replaces the Python
  monkeypatch.
