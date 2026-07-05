# System Overview & Layering

browser-use is a layered, event-driven async-Python library that lets an LLM drive a real
Chromium browser over the Chrome DevTools Protocol (CDP). This document is the entry point:
it maps the eight-layer stack, the peripheral surfaces (MCP, actor), the satellite-services
ring, and the two-control-plane model that ties them together. Every sibling doc drills into
one band of this stack — links are inline and collected at the end.

## The eight-layer stack

Read the system as eight concentric layers. Each is a candidate for isolated extraction (or a
Rust port). A request flows **down** from the agent to the WebSocket; perception data and typed
event results flow **up**.

```
          ┌───────────────────────────────────────────────────────────────┐
   L8     │ LLM providers        BaseChatModel Protocol · ~17 backends      │  browser_use/llm/
          │                      ainvoke() · SchemaOptimizer · JSON repair  │
          ├───────────────────────────────────────────────────────────────┤
   L7     │ Agent orchestrator   step() = perceive→decide→act · AgentOutput │  browser_use/agent/
          │                      MessageManager (3-slot) · multi_act()      │
          ├───────────────────────────────────────────────────────────────┤
   L6     │ Tools / Registry     async fns → dynamic pydantic action models │  browser_use/tools/
          │                      ActionResult · extract → page_extraction_llm│
          ├───────────────────────────────────────────────────────────────┤
   L5     │ DOM perception       DomService fuses DOM+AX+layout → selector_map│  browser_use/dom/
          │                      EnhancedDOMTreeNode · 5-stage serializer    │
          ├───────────────────────────────────────────────────────────────┤
   L4     │ Watchdogs (~14)      single-responsibility bus reactors         │  browser_use/browser/watchdogs/
          │                      downloads·popups·security·dom·screenshot…  │
          ├───────────────────────────────────────────────────────────────┤
   L3     │ BrowserSession       lifecycle + bubus EVENT PLANE (control)     │  browser_use/browser/session.py
          │                      dispatch/event_result RPC-over-bus         │
          ├───────────────────────────────────────────────────────────────┤
   L2     │ SessionManager       single source of truth: targets/sessions   │  browser_use/browser/session_manager.py
          │                      4 dicts under ONE asyncio.Lock             │
          ├───────────────────────────────────────────────────────────────┤
   L1     │ CDP transport        TimeoutWrappedCDPClient over cdp-use       │  browser_use/browser/_cdp_timeout.py
          │                      one multiplexed WebSocket · setAutoAttach  │
          └───────────────────────────────────────────────────────────────┘
                    ▲ raw CDP data plane (send.Domain.method, keyed by sessionId)
```

Dependency direction is strictly top-down: L(n) may call L(n−1) directly, but lower layers never
import upward. The exception is the **event plane** (see below), which lets L4 watchdogs answer
requests originating anywhere without a compile-time dependency.

## Two control planes

The system runs two coordination mechanisms side by side, and the split is the single most
important architectural fact to internalize:

- **Raw-CDP data plane** — direct, imperative, request/response. Any layer with a `CDPSession`
  calls `cdp_client.send.Domain.method(params=..., session_id=...)` and awaits the browser's
  reply. Used for hot-path perception (DOM snapshots, screenshots) and low-level actor scripting.
- **bubus event plane** — asynchronous, decoupled, pub/sub with typed return values. `BrowserSession`
  owns an [`EventBus`](../../browser_use/browser/session.py) (`ResilientEventBus`, session.py:106);
  callers `dispatch()` an event and `await event.event_result(...)` to get a strongly-typed value
  back — a hand-rolled **RPC-over-bus**. Example: `BrowserStateRequestEvent(BaseEvent[BrowserStateSummary])`
  and `ScreenshotEvent(BaseEvent[str])` in [`events.py`](../../browser_use/browser/events.py). See
  session.py:1597 (`await event.event_result(raise_if_none=True, raise_if_any=True)`).

Watchdogs (L4) live entirely on the event plane; they subscribe by naming convention and react.
The DOM/screenshot/action paths reach through to the CDP plane. Detail:
[02-event-bus-and-watchdogs.md](02-event-bus-and-watchdogs.md).

## Module → LOC map

Package `browser_use/` measures **64,735 LOC**; the full repo including tests is **105,767 LOC**.

| Layer / surface | Module | LOC | Doc |
| --- | --- | --- | --- |
| L1 CDP transport | [`browser/_cdp_timeout.py`](../../browser_use/browser/_cdp_timeout.py) | 125 | [01](01-cdp-transport-and-session-manager.md) |
| L2 SessionManager | [`browser/session_manager.py`](../../browser_use/browser/session_manager.py) | 911 | [01](01-cdp-transport-and-session-manager.md) |
| L3 BrowserSession | [`browser/session.py`](../../browser_use/browser/session.py) | 4,046 | [01](01-cdp-transport-and-session-manager.md) |
| L3 events/profile | [`browser/events.py`](../../browser_use/browser/events.py) · [`profile.py`](../../browser_use/browser/profile.py) | 667 · 1,288 | [02](02-event-bus-and-watchdogs.md) |
| L4 watchdogs | [`browser/watchdogs/`](../../browser_use/browser/watchdogs/) (14 files) | ~9,300 | [02](02-event-bus-and-watchdogs.md) |
| L4 base | [`browser/watchdog_base.py`](../../browser_use/browser/watchdog_base.py) | 321 | [02](02-event-bus-and-watchdogs.md) |
| L5 DOM | [`dom/service.py`](../../browser_use/dom/service.py) · [`dom/serializer/serializer.py`](../../browser_use/dom/serializer/serializer.py) · [`dom/views.py`](../../browser_use/dom/views.py) | 1,174 · 1,290 · 1,041 | [03](03-dom-perception-pipeline.md) |
| L6 Tools | [`tools/service.py`](../../browser_use/tools/service.py) · [`tools/registry/service.py`](../../browser_use/tools/registry/service.py) | 2,313 · 601 | [04](04-tools-and-action-registry.md) |
| L7 Agent | [`agent/service.py`](../../browser_use/agent/service.py) · [`agent/views.py`](../../browser_use/agent/views.py) · [`agent/message_manager/service.py`](../../browser_use/agent/message_manager/service.py) | 4,143 · 1,000 · 597 | [05](05-agent-control-loop.md) |
| L8 LLM | [`llm/`](../../browser_use/llm/) (~17 providers) | 9,438 | [06](06-llm-provider-abstraction.md) |
| MCP | [`mcp/`](../../browser_use/mcp/) | 2,129 | [07](07-mcp-integration.md) |
| Actor | [`actor/`](../../browser_use/actor/) | 2,398 | [08](08-actor-scripting-api.md) |
| Beta Rust bridge | [`beta/service.py`](../../browser_use/beta/service.py) | 6,800 | [11](11-beta-rust-bridge.md) |
| Config/logging/CLI | [`config.py`](../../browser_use/config.py) · [`logging_config.py`](../../browser_use/logging_config.py) · [`cli.py`](../../browser_use/cli.py) | 525 · 328 · 323 | [09](09-configuration-logging-bootstrap.md) |
| Satellites | `tokens/` · `filesystem/` · `sandbox/` · `skills/` · `sync/` · `telemetry/` | 1,133 · 941 · 842 · 749 · 524 · 271 | [10](10-cross-cutting-services.md) |

## Layer walk

### L1 — CDP transport

One multiplexed WebSocket carries every CDP call for the whole browser.
[`TimeoutWrappedCDPClient`](../../browser_use/browser/_cdp_timeout.py) subclasses cdp-use's
`CDPClient` and wraps each `send_raw(method, params, session_id)` in `asyncio.wait_for`
(default 60s, `BROWSER_USE_CDP_TIMEOUT_S`). This converts the silent-hang failure mode (TCP
keepalive alive, browser container dead) into a fast `TimeoutError`. The client is created in
`BrowserSession.connect()` (session.py:1820) with `max_ws_frame_size = 200 MB` for huge DOMs.
`setAutoAttach(flatten=True)` makes the browser key every child target's messages by `sessionId`
on the same socket — no per-target connection.

### L2 — SessionManager

[`SessionManager`](../../browser_use/browser/session_manager.py) is the **single source of truth**
for CDP topology. It holds four dicts — `_targets`, `_sessions`, `_target_sessions`,
`_session_to_target` — all mutated under one `asyncio.Lock`. It stays synced with browser reality
by registering `Target.attachedToTarget` / `detachedFromTarget` / `targetInfoChanged` handlers
(session_manager.py:105-107) rather than polling. A target is removed only when its **last** session
detaches. Callers get a session via `BrowserSession.get_or_create_cdp_session(target_id, focus=True)`
(session.py:1448), which adds validation, focus, and recovery on top of the internal accessor.

### L3 — BrowserSession

[`BrowserSession`](../../browser_use/browser/session.py) (a `pydantic.BaseModel`, session.py:134) owns
the CDP root client, the `SessionManager`, and the event bus. It exposes both an event-driven API
(`dispatch(BrowserStartEvent())`, session.py:721) and imperative helpers (`cdp_client` property,
`get_or_create_cdp_session`). `attach_all_watchdogs()` (session.py:1608) instantiates and wires every
watchdog; `_watchdogs_attached` guards against double attach. `is_cdp_connected` (session.py:501)
inspects `ws.state is State.OPEN` and is the circuit breaker used by the watchdog handler wrapper.

### L4 — Watchdogs

~14 single-responsibility services subclass [`BaseWatchdog`](../../browser_use/browser/watchdog_base.py)
(a `pydantic.BaseModel` with `extra='forbid'`). They auto-register handlers by the naming convention
`on_<EventTypeName>` (watchdog_base.py:56-71). Handlers are wrapped so that, if `is_cdp_connected` is
false, they either wait for reconnection or short-circuit — except a `LIFECYCLE_EVENT_NAMES` allowlist
(BrowserStart/Stop/etc., watchdog_base.py:78) that must always run. The heavyweight ones:
[`default_action_watchdog.py`](../../browser_use/browser/watchdogs/default_action_watchdog.py) (3,702 LOC —
click/type/scroll/keys/upload), [`downloads_watchdog.py`](../../browser_use/browser/watchdogs/downloads_watchdog.py)
(1,483), [`dom_watchdog.py`](../../browser_use/browser/watchdogs/dom_watchdog.py) (865 — owns
`BrowserStateRequestEvent`). Others: security, aboutblank, popups, permissions, storage_state,
local_browser, screenshot, recording, har_recording, captcha, crash (commented out).

### L5 — DOM perception

[`DomService`](../../browser_use/dom/service.py) fuses **three CDP trees** into one model. `_get_all_trees()`
(service.py:385) captures `DOMSnapshot.captureSnapshot` (layout), `DOM.getDocument` (structure), and
`Accessibility.getFullAXTree` (semantics), returning a `TargetAllTrees` dataclass (views.py:197):

```python
@dataclass(slots=True)
class TargetAllTrees:
    snapshot: CaptureSnapshotReturns
    dom_tree: GetDocumentReturns
    ax_tree: GetFullAXTreeReturns
    device_pixel_ratio: float
    ...
```

These are merged into `EnhancedDOMTreeNode`s keyed by `backendNodeId`. Then
[`DOMTreeSerializer`](../../browser_use/dom/serializer/serializer.py) runs a 5-stage pipeline in
`serialize_accessible_elements()` (serializer.py:100): build simplified tree + clickable detection →
paint-order removal ([`PaintOrderRemover`](../../browser_use/dom/serializer/paint_order.py)) → filter →
assign interactive indices → emit `SerializedDOMState(selector_map=...)`.

> **CONTRACT:** the selector-map key **is** the `backendNodeId`. See serializer.py:713 —
> `self._selector_map[node.original_node.backend_node_id] = node.original_node`. The LLM emits a
> `backend_node_id`; the tools layer looks it up directly. Detail:
> [03-dom-perception-pipeline.md](03-dom-perception-pipeline.md).

### L6 — Tools & Registry

[`Registry`](../../browser_use/tools/registry/service.py) turns decorated `async` functions into pydantic
action models at runtime. `@registry.action(...)` builds a `<Fn>_Params` model via
`create_model(..., __base__=ActionModel)` (service.py:158); `create_action_model()` (service.py:507)
fuses all registered actions into a `RootModel`-backed discriminated `Union` named `ActionModel`
(service.py:566-588). [`Tools`](../../browser_use/tools/service.py) (`Tools(Generic[Context])`, service.py:441)
registers the concrete browser actions; every action returns the uniform
[`ActionResult`](../../browser_use/agent/views.py). The `extract` action calls the agent's separate
`page_extraction_llm`. Detail: [04-tools-and-action-registry.md](04-tools-and-action-registry.md).

### L7 — Agent orchestrator

[`Agent`](../../browser_use/agent/service.py) drives the loop. `step()` (service.py:1027) has three phases:
**Phase 1** `_prepare_context()` → `get_browser_state_summary(include_screenshot=True)` (perceive);
**Phase 2** `_get_next_action()` → `_execute_actions()` (decide + act); **Phase 3** `_post_process()`.
The action model type is built dynamically per-agent (`AgentOutput.type_with_custom_actions(self.ActionModel)`,
service.py:778-791) so the LLM's structured output deserializes straight into typed actions.
`multi_act()` (service.py:2719) executes an action list with early termination. State/history flows
through the 3-slot [`MessageManager`](../../browser_use/agent/message_manager/service.py). Detail:
[05-agent-control-loop.md](05-agent-control-loop.md).

### L8 — LLM providers

[`BaseChatModel`](../../browser_use/llm/base.py) is a `runtime_checkable` `Protocol` (base.py:18), not an
ABC — providers duck-type it. Its core is the overloaded `async def ainvoke(messages, output_format=None)`
returning `ChatInvokeCompletion[str]` or `ChatInvokeCompletion[T]` (views.py:41). ~17 provider packages
(`openai/`, `anthropic/`, `google/`, `groq/`, `aws/`, `azure/`, `ollama/`, `deepseek/`, `mistral/`,
`cerebras/`, `openrouter/`, `vercel/`, `litellm/`, `oci_raw/`, `browser_use/`, …) implement it, each
handling provider-divergent structured output + JSON repair via `SchemaOptimizer`. Detail:
[06-llm-provider-abstraction.md](06-llm-provider-abstraction.md).

## Peripheral surfaces

- **MCP** ([`mcp/`](../../browser_use/mcp/)) — bidirectional. The **server**
  ([`BrowserUseServer`](../../browser_use/mcp/server.py), server.py:187) exposes browser primitives over
  stdio (`browser_navigate`, `browser_click`, `browser_type`, `browser_get_state`,
  `browser_extract_content`, …; ~16 tools, 14 low-level + 2 LLM-backed). The **client**
  ([`mcp/client.py`](../../browser_use/mcp/client.py)) lets agents consume external MCP servers.
  Detail: [07-mcp-integration.md](07-mcp-integration.md).
- **Actor** ([`actor/`](../../browser_use/actor/)) — an imperative, Playwright-shaped facade
  ([`Page`](../../browser_use/actor/page.py), [`Element`](../../browser_use/actor/element.py)) that talks
  **directly** to `browser_session.cdp_client` (page.py:46), **bypassing the event bus**. It is the
  low-level scripting escape hatch. Detail: [08-actor-scripting-api.md](08-actor-scripting-api.md).

## Satellite-services ring

Cross-cutting concerns that hang off the core rather than sitting in the stack:
config/logging/bootstrap ([09](09-configuration-logging-bootstrap.md)); token accounting
([`tokens/`](../../browser_use/tokens/)); cloud sync ([`sync/`](../../browser_use/sync/)); a filesystem VFS
([`filesystem/`](../../browser_use/filesystem/)); sandboxing ([`sandbox/`](../../browser_use/sandbox/));
telemetry ([`telemetry/`](../../browser_use/telemetry/)); and agent skills
([`skills/`](../../browser_use/skills/)). Detail: [10-cross-cutting-services.md](10-cross-cutting-services.md).

## The beta Rust bridge (authoritative spec)

[`browser_use/beta/service.py`](../../browser_use/beta/service.py) already drives a **native Rust binary**
(`browser-use-terminal`) out-of-process. [`RustSdkClient`](../../browser_use/beta/service.py) (beta/service.py:323)
spawns it via `asyncio.create_subprocess_exec` and speaks **newline-framed JSON-RPC 2.0** over stdin/stdout
(beta/service.py:375, 430). This contract is the closest thing to an authoritative interface spec for any
future Rust core. Detail: [11-beta-rust-bridge.md](11-beta-rust-bridge.md).

## Invariants & gotchas

- **selector_map index == backendNodeId.** Not a synthetic 1..N index — it is the CDP backend node id
  (serializer.py:713). Any renumbering breaks the tools ↔ LLM contract.
- **SessionManager is the only writer of session state.** Everything else reads through
  `get_or_create_cdp_session`. Four dicts, one lock; don't add a parallel cache.
- **CDP timeout < step timeout < agent watchdog.** 60s CDP cap sits well under the 180s step timeout;
  keep that ordering or hangs re-emerge (_cdp_timeout.py docstring).
- **Watchdog handlers can silently no-op when the socket is down** (except lifecycle events). A missing
  side effect is often a dead-WebSocket short-circuit, not a logic bug (watchdog_base.py:98).
- **Action models are built at runtime.** There is no static `ActionModel` enum to grep; it is a per-agent
  `RootModel` `Union` synthesized from the registry (registry/service.py:566).
- **One WebSocket, many targets.** All multiplexing rides `sessionId` from `setAutoAttach(flatten=True)`.
- **`DomService` opens a fresh CDP path per step** (service.py:41 TODO) — a known perf wart, not a design goal.

## Rust port notes

- **Maps cleanly.** L1/L2 are almost a spec already: a `tokio-tungstenite` WebSocket + a `Mutex`-guarded
  topology struct mirrors `TimeoutWrappedCDPClient` + `SessionManager` one-to-one. The beta JSON-RPC bridge
  proves the process boundary works; the Rust core can *be* the `browser-use-terminal` server.
- **Needs redesign.** The **bubus event plane** with typed `event_result()` RPC-over-bus has no idiomatic
  Rust twin — `pydantic` runtime models + naming-convention handler discovery don't translate. Options: an
  enum-typed message bus with `tokio::sync::oneshot` reply channels (typed RPC) plus `mpsc` broadcast for
  reactive watchdogs. The **dynamic pydantic action-model union** (L6) likely becomes a `serde` tagged enum
  generated from a static tool registry — you lose runtime action injection but gain compile-time safety.
- **Hard parts.** The DOM three-tree fusion + 5-stage serializer (L5) is the densest logic and the highest
  bug surface; port it behind a golden-output test harness. The ~17 LLM providers (L8) each carry
  provider-specific structured-output and JSON-repair quirks — treat every provider as its own crate with a
  shared `BaseChatModel`-equivalent trait, and expect the long tail to dominate the effort.

---

See [index.md](index.md) for the soft index and entry points. Sibling docs, in stack order:
[01](01-cdp-transport-and-session-manager.md) ·
[02](02-event-bus-and-watchdogs.md) ·
[03](03-dom-perception-pipeline.md) ·
[04](04-tools-and-action-registry.md) ·
[05](05-agent-control-loop.md) ·
[06](06-llm-provider-abstraction.md) ·
[07](07-mcp-integration.md) ·
[08](08-actor-scripting-api.md) ·
[09](09-configuration-logging-bootstrap.md) ·
[10](10-cross-cutting-services.md) ·
[11](11-beta-rust-bridge.md).
