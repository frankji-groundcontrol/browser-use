# CDP Transport & Session Management

The bottom layer of browser-use: a single multiplexed WebSocket to Chromium (via [cdp-use](https://github.com/browser-use/cdp-use)), wrapped for per-request timeout, plus a [`SessionManager`](../../browser_use/browser/session_manager.py) that is the single source of truth for every CDP target and session. Everything above it — watchdogs, DOM perception, tools, the agent — reaches the browser through this plane. See the [index](index.md) and the [system overview](00-system-overview.md) for how the layers stack.

## The cdp-use send / register facades

cdp-use is a thin, fully-typed wrapper over the raw CDP WebSocket. All CDP traffic goes through two attribute-chained facades on a `CDPClient`:

- **Commands:** `await cdp_client.send.<Domain>.<method>(params=..., session_id=...)` — e.g. `cdp_client.send.Target.setAutoAttach(params={...}, session_id=sid)`. Each call awaits a future that resolves only when Chromium returns a response frame with the matching request id.
- **Events:** `cdp_client.register.<Domain>.<eventName>(callback)` — e.g. `cdp_client.register.Target.attachedToTarget(on_attached)`. Note this is `register.`, **not** `cdp_client.on(...)` (which does not exist).

CDP event callbacks are **synchronous** (a cdp-use requirement), so handlers that need to do async work spawn a task rather than blocking the reader. Throughout [`session_manager.py`](../../browser_use/browser/session_manager.py) this is done with `create_task_with_error_handling(...)` — see `on_attached` / `on_detached` / `on_target_info_changed` in `start_monitoring()`.

cdp-use only owns the socket, framing, and request/response correlation. **All** session bookkeeping, target lifecycle, reconnection, and focus tracking live in browser-use ([`session.py`](../../browser_use/browser/session.py) + [`session_manager.py`](../../browser_use/browser/session_manager.py)).

## One WebSocket, many sessions: the flatten model

There is exactly one socket per `BrowserSession`, stored as `self._cdp_client_root` ([`session.py:555`](../../browser_use/browser/session.py)). Every page, iframe (including OOPIFs), and worker is driven over that same connection. The mechanism is CDP's flattened auto-attach:

```python
await self._cdp_client_root.send.Target.setAutoAttach(
    params={'autoAttach': True, 'waitForDebuggerOnStart': False, 'flatten': True}
)
```

`flatten=True` means child-target traffic is multiplexed over the root socket and demultiplexed by a `sessionId` field on each frame, instead of opening a nested socket per target. Auto-attach is **not** recursive by default: the root call only covers top-level targets, so `_handle_target_attached` re-issues `setAutoAttach(..., flatten=True, session_id=session_id)` on **each** newly attached session so its children (nested iframes, workers) attach too ([`session_manager.py:394`](../../browser_use/browser/session_manager.py)). The socket is created with `max_ws_frame_size=200 * 1024 * 1024` (200 MB) to survive very large DOM snapshots.

## TimeoutWrappedCDPClient

[`_cdp_timeout.py`](../../browser_use/browser/_cdp_timeout.py) subclasses `cdp_use.CDPClient` to defend against a specific failure mode: a remote/cloud browser whose WebSocket stays alive at the TCP/keepalive layer while the browser container is dead or a proxy has lost its upstream. cdp-use's `send_raw()` awaits a future that would never resolve, hanging the whole agent.

```python
class TimeoutWrappedCDPClient(CDPClient):
    async def send_raw(self, method, params=None, session_id=None) -> dict[str, Any]:
        try:
            return await asyncio.wait_for(
                super().send_raw(method=method, params=params, session_id=session_id),
                timeout=self._cdp_request_timeout_s,
            )
        except TimeoutError as e:
            raise TimeoutError(f'CDP method {method!r} did not respond within ...') from e
```

- **Cap:** `DEFAULT_CDP_REQUEST_TIMEOUT_S` = 60 s, from `BROWSER_USE_CDP_TIMEOUT_S` (process-wide) or the `cdp_request_timeout_s=` constructor arg. Both paths pass through defensive parsers (`_parse_env_cdp_timeout`, `_coerce_valid_timeout`) that reject non-finite / non-positive values and fall back to 60 s with a warning — a bad value would otherwise make every CDP call time out immediately (`nan`) or never (`inf`/`0`/negative).
- **Why 60 s:** generous for slow ops like `Page.captureScreenshot` / `Page.printToPDF` on heavy pages, but well below the 180 s agent step timeout, so a silent hang surfaces as a fast, catchable `TimeoutError` that flows through existing `except TimeoutError` paths.

Both `connect()` and `reconnect()` instantiate this subclass, never the raw `CDPClient`.

## SessionManager: four dicts under one lock

[`SessionManager`](../../browser_use/browser/session_manager.py) is declared the **single source of truth** for all targets and sessions. Its state is four dicts:

| Field | Type | Meaning |
|---|---|---|
| `_targets` | `dict[TargetID, Target]` | entities: pages, iframes, workers |
| `_sessions` | `dict[SessionID, CDPSession]` | communication channels |
| `_target_sessions` | `dict[TargetID, set[SessionID]]` | forward: a target's attached sessions |
| `_session_to_target` | `dict[SessionID, TargetID]` | reverse index |

Two data types back these (both `BaseModel`, `revalidate_instances='never'`, in [`session.py`](../../browser_use/browser/session.py)): `Target{target_id, target_type, url, title}` and `CDPSession{cdp_client, target_id, session_id}` plus private `_lifecycle_events` (a `deque(maxlen=50)`) and `_lifecycle_lock`.

**Invariants:**
- All four dicts are mutated **only** under `self._lock` (a single `asyncio.Lock`), and **only** inside `_handle_target_attached` (the sole insertion point) and `_handle_target_detached` (the sole removal point). Reads elsewhere are lock-free snapshots.
- Multiple sessions may attach to one target; a target is removed **only when its session set empties** (`remaining_sessions == 0`). This is why detach decrements before deleting.
- Target creation and session-set insertion happen inside the same lock acquisition, so `get_target()` is never called in the window between `_target_sessions` and `_targets` being set.
- `_handle_target_attached` is idempotent (checks `if target_id not in self._targets`), so the startup discovery path and Chrome's own `attachedToTarget` event can both fire without double-counting.

`start_monitoring()` bootstraps this: it enables `Target.setDiscoverTargets`, registers the three event handlers, then calls `_initialize_existing_targets()` which `Target.attachToTarget`s every pre-existing target and waits (event-driven, 2 s cap) for their sessions + page monitoring to come up. For page/tab targets, `_enable_page_monitoring` turns on `Page`/`Network` domains and registers **one** `Page.lifecycleEvent` handler per session that appends into the per-session `_lifecycle_events` deque — navigations later consume these instead of re-registering handlers (avoids handler accumulation).

`_handle_target_detached` also dispatches a `TabClosedEvent` on the bus, but only for fully-removed `page`/`tab` targets (not iframes/workers, not partial detaches). This is the seam into the [event-bus / watchdog layer](02-event-bus-and-watchdogs.md).

## get_or_create_cdp_session: target_id → agent-focus resolver

`get_or_create_cdp_session(target_id: TargetID | None = None, focus: bool = True) -> CDPSession` ([`session.py:1448`](../../browser_use/browser/session.py)) is the primary way the whole codebase obtains a session. Flow:

1. If `target_id is None`, call `session_manager.ensure_valid_focus(timeout=5.0)`; on success use `self.agent_focus_target_id`, else `ValueError`.
2. Look up the session via `_get_session_for_target(target_id)`. If absent (attach event not yet processed), poll 20 × 0.1 s waiting for it; still absent → `ValueError('Target ... not found')`.
3. `validate_session(target_id)` — must still have ≥1 live session.
4. If `focus=True` **and** the target's `target_type == 'page'`, set `agent_focus_target_id = target_id`. Focus is **refused for iframe/worker targets** — they can detach at any moment, which would leave focus dangling.
5. Best-effort `Runtime.runIfWaitingForDebugger` (3 s cap, failures ignored).

The internal `_get_session_for_target` picks an arbitrary session from the target's set (`next(iter(session_ids))`) and, as defense-in-depth, triggers focus recovery if it is asked for a stale focused target. Prefer the public method — the internal one has no validation, focus, or wait.

## cdp_client_for_node: the four-strategy resolver

Because a CDP `backendNodeId` is **only valid in the session where the DOM was captured**, resolving a session for a specific DOM node cannot just use agent focus. `cdp_client_for_node(node: EnhancedDOMTreeNode)` ([`session.py:3867`](../../browser_use/browser/session.py)) trusts the node's own recorded identity and tries, in order:

1. **`node.session_id`** → `session_manager.get_session(session_id)` (most specific, exact channel).
2. **`node.frame_id`** → `cdp_client_for_frame(frame_id)`, which builds a unified frame hierarchy across all targets (`get_all_frames()` / `find_frame_target()`) to locate the owning target, correctly handling OOPIFs. When `cross_origin_iframes` is disabled it short-circuits to the main session.
3. **`node.target_id`** → `get_or_create_cdp_session(target_id, focus=False)`.
4. **`agent_focus_target_id`** → the page the agent is currently working on (logged as a warning — the node lacked identity).

Last resort: `get_or_create_cdp_session()` (main session, logged as an error). This maps cleanly to the DOM layer's contract that indices are `backendNodeId`s scoped to a capture session — see [DOM perception](03-dom-perception-pipeline.md).

## Connect lifecycle

`connect(cdp_url=None)` ([`session.py:1759`](../../browser_use/browser/session.py)) must succeed or the browser is unusable (fails hard). Sequence:

1. Resolve `cdp_url`. If it is HTTP, GET `/json/version` and read `webSocketDebuggerUrl` (localhost disables `trust_env` to dodge proxy env vars).
2. Build `TimeoutWrappedCDPClient(cdp_url, additional_headers=..., max_ws_frame_size=200MB)`, `await .start()`.
3. **SessionManager first, autoAttach second.** `SessionManager(self).start_monitoring()` registers handlers and discovers existing targets *before* the root `setAutoAttach` is enabled, so no attach event is ever missed (any race is absorbed by idempotent handling).
4. Redirect any `chrome://newtab` pages to `about:blank`; ensure ≥1 page exists (create one if not); set initial focus via `get_or_create_cdp_session(target_id, focus=True)`.
5. `_setup_proxy_auth()` (registers `Fetch.authRequired`/`requestPaused` handlers if proxy creds are set), then `_intentional_stop = False` and `_attach_ws_drop_callback()`.
6. Dispatch `TabCreatedEvent` per initial tab and an `AgentFocusChangedEvent`.

On any exception it clears the SessionManager, stops the socket, nulls `_cdp_client_root` / `session_manager` / `agent_focus_target_id`, and re-raises as `RuntimeError`.

## WS-drop detection & auto-reconnect

**Detection** — `_attach_ws_drop_callback()` ([`session.py:2225`](../../browser_use/browser/session.py)) hooks a done-callback onto cdp-use's internal `_message_handler_task`. That task exiting means the read loop died, i.e. the socket dropped. The callback ignores the event if `_intentional_stop`, already `_reconnecting`, or no `cdp_url`; otherwise it schedules `_auto_reconnect()`.

**`_auto_reconnect(max_attempts=3)`** — guarded by `_reconnect_lock` + the `_reconnecting` flag against concurrent callers. Backoff `delays = [1.0, 2.0, 4.0]`; each attempt is `asyncio.wait_for(self.reconnect(), timeout=15.0)`. It dispatches `BrowserReconnectingEvent` before each try, `BrowserReconnectedEvent` on success, `BrowserErrorEvent('ReconnectionFailed')` if all exhaust. `finally` clears `_reconnecting` and `_reconnect_event.set()` to wake all waiters regardless of outcome.

**`reconnect()`** — a full teardown/rebuild against the *same* `cdp_url`: stop old client, `session_manager.clear()`, null focus, new `TimeoutWrappedCDPClient`, new `SessionManager` + `start_monitoring`, re-enable root `setAutoAttach`, restore the previous focus target if it still exists (else `page_targets[0]`, else create a blank page), re-run proxy auth, re-attach the drop callback.

**Consumers block, they don't fail.** `is_reconnecting` + `_reconnect_event` + `RECONNECT_WAIT_TIMEOUT = 54.0` (= 3×15 + (1+2+4) + 2 buffer) let callers wait out a reconnect. The [watchdog base](../../browser_use/browser/watchdog_base.py) has a circuit breaker: if `not is_cdp_connected` and `is_reconnecting`, it `await`s `_reconnect_event` (up to 54 s) before running a handler; the [agent loop](../../browser_use/agent/service.py) does the same at step boundaries. `is_cdp_connected` verifies the underlying `ws.state is State.OPEN`, so a closing/closed socket reports disconnected immediately rather than hanging.

## Agent-focus recovery state machine

`agent_focus_target_id` (a plain field on `BrowserSession`) names the page the agent drives. When that target detaches, `_handle_target_detached` clears the field and — if not already running — spawns `_recover_agent_focus(crashed_target_id)`. Recovery coordination state lives on the SessionManager: `_recovery_in_progress: bool`, `_recovery_complete_event: asyncio.Event | None`, `_recovery_lock: asyncio.Lock`, `_recovery_task`.

`_recover_agent_focus` ([`session_manager.py:602`](../../browser_use/browser/session_manager.py)):
1. Under `_recovery_lock`, set `_recovery_in_progress` and a fresh `_recovery_complete_event`. If recovery is already running, await the existing event instead of starting a second one (prevents spawning duplicate emergency tabs).
2. Pick a new target: the most-recent existing page (`get_all_page_targets()[-1]`), or `_cdp_create_new_page('about:blank')` if none remain (dispatching `TabCreatedEvent`).
3. Poll 20 × 0.1 s for Chrome to fire `attachedToTarget` and populate the session (this poll is genuinely necessary — it waits on an external browser event). On success set focus, `Target.activateTarget` for existing tabs, dispatch `AgentFocusChangedEvent`.
4. If that fails, create an emergency fallback tab and retry once; total failure logs `CRITICAL`.
5. `finally`: `_recovery_complete_event.set()`, reset `_recovery_in_progress`, clear `_recovery_task`.

`ensure_valid_focus(timeout=3.0)` is the consumer side: it checks whether the focused target still has a session; if stale, it waits on `_recovery_complete_event` (event-driven, not polling) rather than racing. `get_or_create_cdp_session(target_id=None)` calls it with a 5 s timeout, so any code path that asks for "the current page" transparently blocks through a focus loss and resumes on the recovered tab.

## Gotchas

- **AutoAttach must be re-issued per session** for nested children; the root-level call is not recursive.
- **SessionManager before autoAttach**, always — reordering reintroduces the missed-attach race.
- **`backendNodeId` is session-scoped.** Never reuse a node across sessions; go through `cdp_client_for_node`.
- **Focus is page-only.** `get_or_create_cdp_session(focus=True)` silently ignores focus requests for iframes/workers.
- **The 60 s CDP cap is per request, not per step.** Very heavy `printToPDF` on huge pages can legitimately need `BROWSER_USE_CDP_TIMEOUT_S` raised.
- **Recovery polling loops** (`20 × 0.1 s`) are intentional: they wait on external Chrome CDP events, not on internal state.

## Rust port notes

- **Socket + framing:** `tokio-tungstenite` for the WS; a single reader task decoding frames and routing by `(id, sessionId)` to oneshot channels. `chromiumoxide`'s handler is a decent reference; hand-rolling keeps the flatten/session model explicit.
- **Timeout wrapper → trivial.** `tokio::time::timeout(dur, send)` per request replaces `TimeoutWrappedCDPClient` one-for-one; env parsing is the same defensive clamp.
- **Four dicts + one lock → an owning task.** The cleanest Rust shape is an actor: a task owning the four `HashMap`s, fed commands over an `mpsc`, replying via `oneshot`. This removes the `asyncio.Lock` entirely and makes the "single mutation point" invariant structural. `Arc<Mutex<SessionManager>>` also works but re-creates the lock-ordering care the Python takes.
- **Sync CDP callbacks → messages.** Python spawns tasks from sync event handlers; in Rust the reader task simply forwards `Target.attachedToTarget` / `detachedFromTarget` as messages to the SessionManager actor — no callback re-entrancy problem.
- **Drop detection is cleaner.** Watching cdp-use's `_message_handler_task` done-callback maps to awaiting the reader `JoinHandle` or observing a WS close frame. Backoff is a plain loop with `tokio::time::sleep`.
- **Recovery state machine → enum + `Notify`.** `_recovery_in_progress`/`_recovery_complete_event` become a small state enum plus a `tokio::sync::Notify` (or a `watch` channel) that waiters subscribe to.
- **Hard parts:** faithfully reproducing per-session lifecycle-event buffering, OOPIF frame-hierarchy resolution (`cdp_client_for_frame`), and the exact idempotency of startup discovery vs. live attach events. The bus-facing dispatches (`TabClosedEvent`, `AgentFocusChangedEvent`) are the contract with layer 3 — see the [beta Rust bridge](11-beta-rust-bridge.md), whose JSON-RPC framing is the authoritative spec for a Rust core.
