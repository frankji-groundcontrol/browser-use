# The bubus Event Bus & Watchdog Services

The control plane of a `BrowserSession` is an in-process async event bus (the third-party [`bubus`](https://github.com/browser-use/bubus) library) over which ~14 single-responsibility **watchdog** services react to typed events. The bus doubles as an RPC mechanism: some events carry a typed return value that the dispatcher `await`s, so "take a screenshot" or "give me the browser state" is a `dispatch()` + `await event.event_result()` round-trip rather than a direct method call.

This layer sits *above* the raw CDP transport ([`01-cdp-transport-and-session-manager.md`](01-cdp-transport-and-session-manager.md)) and *below* the Tools/Agent layers ([`04-tools-and-action-registry.md`](04-tools-and-action-registry.md), [`05-agent-control-loop.md`](05-agent-control-loop.md)). See the [index](index.md) for the full set.

---

## bubus in two sentences

bubus gives you `BaseEvent` (a pydantic model) and `EventBus`. You `bus.on(EventClass, handler)` to subscribe and `bus.dispatch(EventClass(...))` to enqueue; dispatch returns the *same* event object, which is awaitable — awaiting it blocks until every matching handler has run, and `await event.event_result()` returns the first handler's typed return value.

The library lives outside this repo (a pinned dependency), but its behavior is load-bearing, so the mechanics are documented here. Symbols referenced: `bubus.service.EventBus`, `bubus.models.BaseEvent`, `bubus.models.EventResult`.

## The event model: `BaseEvent[T]`

Every browser event subclasses `BaseEvent[T]`, where `T` is the *result type* returned by handlers. Defined in [`browser_use/browser/events.py`](../../browser_use/browser/events.py):

```python
class ScreenshotEvent(BaseEvent[str]): ...                       # handler returns a base64 str
class BrowserStateRequestEvent(BaseEvent[BrowserStateSummary]): ...  # handler returns a pydantic model
class NavigateToUrlEvent(BaseEvent[None]): ...                   # fire-and-forget
class SwitchTabEvent(BaseEvent[TargetID]): ...
```

`T` is auto-extracted from the `BaseEvent[T]` generic parameter at class-creation time (`_extract_basemodel_generic_arg`, cached per class) and stored as `event_result_type`; `EventResult.update()` then validates/coerces each handler's return value against it — pydantic `model_validate` for `BaseModel` subclasses, `TypeAdapter` otherwise. A coercion failure marks that result `status='error'` rather than raising inline.

Key `event_`-prefixed metadata fields (all metadata is prefixed `event_` to avoid colliding with subclass data fields — an assertion in `bubus.models` enforces this):

- `event_id: UUIDStr` — uuid7 (time-sortable), `event_type: str` (defaults to the class name), `event_schema: str` (`module.Qualname@version`).
- `event_timeout: float | None` — per-event handler timeout, **default 300 s**. Browser events override it per-class via `_get_timeout('TIMEOUT_<EventName>', default)`, which reads an env var and falls back to a hardcoded default (e.g. `NavigateToUrlEvent` = 30 s, `ClickElementEvent` = 15 s, `ScrollEvent` = 8 s). This makes every handler timeout individually tunable via environment.
- `event_parent_id`, `event_path: list[str]` — causal + routing metadata (see below).
- `event_results: dict[handler_id, EventResult]` — one `EventResult` per handler that ran, keyed by `f'{id(bus)}.{id(handler)}'`.
- `_event_completed_signal: asyncio.Event` — lazily created; set when all handlers (and all child events) are done. `await event` waits on it.

## Dispatch and the run loop

`EventBus.dispatch(event)` is **synchronous**: it validates the event, sets `event_parent_id`/`event_path`, `put_nowait`s it onto a `CleanShutdownQueue`, records it in `event_history`, and returns the same object immediately. A background `_run_loop()` task (auto-started on first dispatch) pulls events one at a time via `step()`.

Processing is **serial across events, parallel-or-serial across handlers**:

- A process-global re-entrant lock (`ReentrantLock` backed by a `ContextVar`, shared by *all* `EventBus` instances in the process) is held for the duration of each event's processing. So even across multiple buses (Agent bus + Session bus), only one event is ever being processed at a time — this is the concurrency model's backbone and the reason handlers can safely mutate shared session state without their own locks.
- Within an event, `_execute_handlers` runs handlers serially by default (`parallel_handlers=False`); browser-use relies on this. Each handler runs under `asyncio.wait_for(..., timeout=event_result.timeout)`, wrapped by a 15 s "deadlock monitor" that logs a warning if a handler hangs.

`await event` returns the completed event. Crucially, if you await an event *from inside another handler* (nested RPC), `BaseEvent.__await__` detects it holds the global lock and **pumps the queues inline** (processing child events until the awaited one completes) rather than deadlocking on the lock it already owns.

## RPC-over-bus: `event_result()` and friends

The RPC pattern is: dispatch a typed event, await its result. From [`browser_use/browser/session.py`](../../browser_use/browser/session.py) (`get_browser_state_summary`):

```python
event = self.event_bus.dispatch(BrowserStateRequestEvent(include_dom=True, ...))
result = await event.event_result(raise_if_none=True, raise_if_any=True)  # -> BrowserStateSummary
```

`event_result()` (and the plural variants `event_results_list`, `event_results_by_handler_name`, `event_results_flat_dict`, `event_results_flat_list`) all funnel through `event_results_filtered`, whose two flags govern error semantics:

- `raise_if_any=True` — if *any* handler recorded an error, re-raise the first one (original exception type/traceback preserved). This is how a watchdog handler's exception propagates back to the `await`ing caller.
- `raise_if_none=True` — if *no* handler returned a "truthy" result, raise `ValueError`. Truthy = `status=='completed'`, non-`None`, not an exception, and **not a `BaseEvent`** (forwarded events are excluded from result aggregation).

So `raise_if_none=True, raise_if_any=True` means "exactly-one-real-answer-or-explode" (used for `BrowserStateRequestEvent`, `BrowserLaunchEvent`), while lifecycle dispatches use `raise_if_none=False` (fire-and-forget where zero handlers is fine).

## Causal parenting, `event_path`, and loop prevention

bubus tracks causality automatically via `ContextVar`s set during handler execution:

- `_current_event_context` — the event currently being handled. Any `dispatch()` inside a handler inherits it as `event_parent_id`, and the child is appended to the parent handler's `EventResult.event_children`. A parent event is not marked complete until all its handlers *and* all transitively-dispatched children are complete (`event_are_all_children_complete`).
- `event_path` — the list of bus names the event has traversed. Used to forward events between buses (`bus.on('*', other_bus.dispatch)`) without infinite loops: `_would_create_loop` refuses to forward to a bus already in the path.
- Non-forwarding recursion is bounded: `_handler_dispatched_ancestor` walks the parent chain, and if the *same* handler appears >2 levels deep it raises `RuntimeError('Infinite loop detected')`.

This causal tree is what powers the "↲ triggered by … / ⤴ returned to …" debug logs the watchdog wrapper prints.

## History, WAL, and back-pressure

- `event_history: dict[event_id, BaseEvent]` retains the last `max_history_size` events (default **50**), pruned by `cleanup_event_history()` which evicts completed events first, then started, then pending.
- Hard back-pressure: `dispatch()` raises `RuntimeError('EventBus at capacity')` if queued + in-flight pending events reach **100**. The queue itself is bounded at `maxsize=50` when history limits are on.
- Optional `wal_path` writes each completed event as a JSONL line (`_default_wal_handler`) — off by default in browser-use.
- A cross-instance memory guard warns if total event payload across all buses exceeds 50 MB.

## `ResilientEventBus`

`BrowserSession.event_bus` defaults to `ResilientEventBus` ([`browser_use/browser/session.py`](../../browser_use/browser/session.py), ~line 106), a thin subclass whose only job is to make `step()`/`wait_until_idle()` **no-op** when the bus's async primitives (`_on_idle`, `event_queue`) have been nulled out. `Agent.close()` tears down a `keep_alive` session's bus to release the event loop; on a warm-Lambda resume the worker may `step()` it before a `dispatch()` restarts it, and stock bubus would assert `_start() must be called before step()` (regression ENG-5280). The subclass also preserves the `EventBus_<suffix>` naming convention bubus would otherwise derive from the class name.

---

## `BaseWatchdog`: reflection-based registration

All watchdogs subclass `BaseWatchdog` ([`browser_use/browser/watchdog_base.py`](../../browser_use/browser/watchdog_base.py)), a pydantic model holding two dependencies: `event_bus: EventBus` and `browser_session: BrowserSession`. They declare two class vars for documentation/enforcement:

```python
LISTENS_TO: ClassVar[list[type[BaseEvent]]]   # events this watchdog handles
EMITS: ClassVar[list[type[BaseEvent]]]        # events it dispatches
```

`attach_to_session()` uses **reflection**: it scans `dir(self)` for methods named `on_<EventName>`, matches `<EventName>` against the event classes exported by `browser_use.browser.events`, and registers each via `attach_handler_to_session`. Two assertions keep the naming contract honest: a handler `on_Foo` must correspond to a real `FooEvent` and, if `LISTENS_TO` is non-empty, every handled event must be declared in it (and vice-versa, warned).

### The handler wrapper: circuit breaker + error recovery

`attach_handler_to_session` does *not* register the bare method. It wraps it in a `unique_handler` closure (uniquely named `<WatchdogClass>.on_<Event>` to dodge bubus's duplicate-handler warning) that adds three behaviors:

1. **CDP circuit breaker.** Before running, if `event.event_type` is *not* a lifecycle event (`BrowserStartEvent`, `BrowserStopEvent`, `BrowserLaunchEvent`, `BrowserKillEvent`, the reconnect/stopped/error events — a hardcoded `LIFECYCLE_EVENT_NAMES` frozenset) and `browser_session.is_cdp_connected` is `False`, the handler is short-circuited. `is_cdp_connected` checks the root CDP client's WebSocket is in `State.OPEN`. This prevents handlers from dispatching CDP commands that would hang until timeout on a dead socket. If a reconnection is in progress (`is_reconnecting`), it instead `await`s `_reconnect_event` (up to `RECONNECT_WAIT_TIMEOUT` = 54 s) and proceeds if it succeeds, else raises `ConnectionError`.
2. **Structured debug logging.** Emits the `🚌 [Watchdog.on_Event(#id)] ⏳ Starting… ↲ triggered by …` / `Succeeded (Xs)` / `❌ Failed` lines, reconstructing the causal chain from `event_parent_id`.
3. **CDP session self-repair.** On handler exception it attempts `get_or_create_cdp_session(target_id=..., focus=True)` to heal a crashed CDP session, then **re-raises the original error** (so `raise_if_any` still surfaces it to the caller).

The lifecycle-event exemption is essential: `BrowserStartEvent`/`BrowserLaunchEvent` must run *precisely when* CDP is not yet connected.

Watchdogs are all instantiated and attached once, in order, by `BrowserSession.attach_all_watchdogs()` (guarded by `_watchdogs_attached`). Ordering matters — `LocalBrowserWatchdog` must be listening before `BrowserLaunchEvent` fires, and `DOMWatchdog` depends on `ScreenshotWatchdog`.

## The ~14 watchdogs and their wiring

Fourteen `BaseWatchdog` subclasses exist in [`browser_use/browser/watchdogs/`](../../browser_use/browser/watchdogs/); `CrashWatchdog` is defined but currently commented out of `attach_all_watchdogs`. `StorageStateWatchdog`, `HarRecordingWatchdog`, and `CaptchaWatchdog` attach conditionally (on `user_data_dir`/`storage_state`, `record_har_path`, and captcha support respectively).

| Watchdog | LISTENS_TO | EMITS | Responsibility |
|---|---|---|---|
| [`LocalBrowserWatchdog`](../../browser_use/browser/watchdogs/local_browser_watchdog.py) | BrowserLaunch, BrowserKill, BrowserStop | — | Spawns/kills the local Chromium subprocess; returns `BrowserLaunchResult(cdp_url=…)` |
| [`SecurityWatchdog`](../../browser_use/browser/watchdogs/security_watchdog.py) | NavigateToUrl, NavigationComplete, TabCreated | BrowserError | Enforces `allowed_domains`/`prohibited_domains` |
| [`AboutBlankWatchdog`](../../browser_use/browser/watchdogs/aboutblank_watchdog.py) | BrowserStop, BrowserStopped, TabCreated, TabClosed | NavigateToUrl, CloseTab, AboutBlankDVDScreensaverShown | Keeps ≥1 about:blank tab; DVD animation |
| [`DownloadsWatchdog`](../../browser_use/browser/watchdogs/downloads_watchdog.py) | BrowserLaunch, BrowserStateRequest, BrowserStopped, TabCreated, TabClosed, NavigationComplete | DownloadStarted, DownloadProgress, FileDownloaded | PDF auto-download, CDP + network download capture |
| [`StorageStateWatchdog`](../../browser_use/browser/watchdogs/storage_state_watchdog.py) | BrowserConnected, BrowserStop, SaveStorageState, LoadStorageState | StorageStateSaved, StorageStateLoaded | Cookie/localStorage persistence |
| [`PopupsWatchdog`](../../browser_use/browser/watchdogs/popups_watchdog.py) | TabCreated | — | Auto-accepts/dismisses JS dialogs (alert/confirm/prompt/beforeunload) |
| [`PermissionsWatchdog`](../../browser_use/browser/watchdogs/permissions_watchdog.py) | BrowserConnected | — | Grants browser permissions (clipboard, camera, …) |
| [`DefaultActionWatchdog`](../../browser_use/browser/watchdogs/default_action_watchdog.py) | Click/Type/Scroll/GoBack/GoForward/Refresh/Wait/SendKeys/UploadFile/ScrollToText/… | — | The bulk of interaction actions |
| [`ScreenshotWatchdog`](../../browser_use/browser/watchdogs/screenshot_watchdog.py) | ScreenshotEvent | — | Returns base64 screenshot (RPC) |
| [`DOMWatchdog`](../../browser_use/browser/watchdogs/dom_watchdog.py) | TabCreated, BrowserStateRequest | BrowserError | Builds the DOM tree + `BrowserStateSummary` (RPC); see [`03-dom-perception-pipeline.md`](03-dom-perception-pipeline.md) |
| [`RecordingWatchdog`](../../browser_use/browser/watchdogs/recording_watchdog.py) | BrowserConnected, BrowserStop, AgentFocusChanged | — | Video (screencast) recording |
| [`HarRecordingWatchdog`](../../browser_use/browser/watchdogs/har_recording_watchdog.py) | BrowserConnected, BrowserStop | — | HAR network capture |
| [`CaptchaWatchdog`](../../browser_use/browser/watchdogs/captcha_watchdog.py) | BrowserConnected, BrowserStopped | CaptchaSolverStarted, CaptchaSolverFinished | Bridges proxy captcha-solver CDP events |
| [`CrashWatchdog`](../../browser_use/browser/watchdogs/crash_watchdog.py) *(disabled)* | (target crash / network hang) | BrowserError | Detects target crashes + hung requests |

Note: `DefaultActionWatchdog` declares no `LISTENS_TO`, relying purely on the `on_<Event>` reflection to register its ~dozen handlers.

## The raw-CDP-callback → bubus bridge

CDP is push-based: Chrome fires protocol events (`Browser.downloadWillBegin`, `Target.targetCrashed`, `Page.javascriptDialogOpening`, …) into the multiplexed WebSocket. Watchdogs subscribe to these with cdp-use's `cdp_client.register.<Domain>.<event>(callback)` (**not** bubus's `on`), then translate the raw callback into a bus `dispatch()`. Example from [`downloads_watchdog.py`](../../browser_use/browser/watchdogs/downloads_watchdog.py):

```python
cdp_client.register.Browser.downloadWillBegin(download_will_begin_handler)
cdp_client.register.Browser.downloadProgress(download_progress_handler)
# …inside the callback:
self.event_bus.dispatch(FileDownloadedEvent(guid=guid, url=..., path=..., ...))
```

Same pattern in [`crash_watchdog.py`](../../browser_use/browser/watchdogs/crash_watchdog.py) (`register.Target.targetCrashed`), [`popups_watchdog.py`](../../browser_use/browser/watchdogs/popups_watchdog.py) (`register.Page.javascriptDialogOpening`), and [`captcha_watchdog.py`](../../browser_use/browser/watchdogs/captcha_watchdog.py) (`register.BrowserUse.captchaSolver*`). This bridge is the boundary between the raw CDP plane and the bubus control plane: external CDP callbacks are the *sources* of many events, while agent/tool `dispatch()` calls are the other source. Session-lifecycle CDP callbacks (WebSocket drop → reconnect) similarly dispatch `BrowserReconnectingEvent`/`BrowserReconnectedEvent`/`BrowserErrorEvent` (see `_attach_ws_drop_callback` and the reconnect loop in [`session.py`](../../browser_use/browser/session.py)).

## Invariants & gotchas

- **One event at a time, process-wide.** The global re-entrant lock serializes all event processing across every bus. Handlers therefore need no internal locking for session state, but a slow handler stalls *everything* — hence per-event `event_timeout` and the 15 s deadlock monitor.
- **`dispatch()` is sync, `await event` is async.** Forgetting to await means the event is queued but you never see its result or exceptions.
- **`raise_if_none=True` on a fire-and-forget event throws.** Only use it when a handler actually returns a value.
- **Circuit breaker silently returns `None`** for non-lifecycle handlers when CDP is down and not reconnecting — a handler "not running" is not an error, so `raise_if_none=False` callers must tolerate it.
- **Handler names must be unique per event** (`<Watchdog>.on_<Event>`); double-attaching a watchdog raises `RuntimeError('Duplicate handler registration')`.
- **Result coercion is silent-to-error, not silent-to-pass:** a handler returning the wrong type for `BaseEvent[T]` yields an `error` result, surfaced only via `raise_if_any`.
- **Event names may not be substrings of one another** (`_check_event_names_dont_overlap` in `events.py`) — protects grep/sed refactors.

---

## Rust port notes

- **The bus maps cleanly onto `tokio` + channels.** `dispatch()` → `mpsc::UnboundedSender<Event>`; the run loop → a task `select!`ing on the receiver. The awaitable-event / RPC pattern (`await event.event_result()`) maps to a `oneshot::Sender<Result<T>>` embedded in the event, resolved by the handler — this is cleaner in Rust than bubus's `asyncio.Event` + `EventResult` bookkeeping.
- **Typed result per event.** `BaseEvent[T]` is a natural fit for an `enum Event { Screenshot(ScreenshotReq), BrowserState(StateReq), … }` where each variant owns its `oneshot::Sender<T>`. This trades bubus's runtime pydantic coercion for compile-time exhaustiveness — a net win, but it means the dynamic "dispatch an arbitrary runtime-defined event" flexibility is lost (acceptable; browser-use never does that).
- **The global re-entrant lock is the hard part.** bubus serializes *all* handlers across *all* buses via one process-global `ContextVar` lock, and supports re-entrant inline queue-pumping when a handler awaits a child event. In Rust, prefer a single actor task owning session state (message-passing, no shared lock) over replicating the re-entrant lock; the inline-pump-to-avoid-deadlock trick is a symptom of shared-lock design and should not be ported. Crates: `tokio` (runtime), `tokio::sync::{mpsc, oneshot}`, optionally `flume`.
- **Reflection-based `on_<Event>` registration** does not port — use a `trait Watchdog { fn handles(&self) -> &[EventKind]; async fn handle(&mut self, ev: &Event); }` with an explicit match, or a macro. `LISTENS_TO`/`EMITS` become associated consts checked at compile time.
- **The CDP-callback → bus bridge** stays: the CDP client's event stream (see [`01-cdp-transport-and-session-manager.md`](01-cdp-transport-and-session-manager.md)) feeds a task that translates protocol events into `Event`s on the same channel. `serde` replaces pydantic for (de)serialization; the WAL becomes a `serde_json` JSONL sink.
- **Causal parenting / loop detection** (`event_parent_id`, `event_path`, recursion-depth guard) is largely a debugging affordance; a Rust port can keep a lightweight `parent_id: Option<Uuid>` for tracing (`tracing` spans are the idiomatic replacement) and drop the runtime loop-detection machinery if the actor model precludes the loops it guards against.
- The **beta Rust bridge** ([`11-beta-rust-bridge.md`](11-beta-rust-bridge.md)) already drives a native binary over JSON-RPC and is the authoritative spec for the wire contract a Rust core must satisfy.
