# MCP Server & Client Integration

browser-use speaks the [Model Context Protocol](https://modelcontextprotocol.io) in **both directions**: as a *server* it exposes 16 browser-automation tools over stdio JSON-RPC so an external agent (Claude Desktop, Claude Code, Codex) can drive a real Chromium; as a *client* it connects to arbitrary external MCP servers and folds their tools into the local action [Registry](04-tools-and-action-registry.md) so the browser-use Agent can call them. Both halves live in [`browser_use/mcp/`](../../browser_use/mcp/) and are thin adapters over the official `mcp` Python SDK.

See the operator-facing walkthrough in [usage/tools/mcp-multi-agent-setup.md](../usage/tools/mcp-multi-agent-setup.md). For the sibling layers this glues together, see [Tools & Action Registry](04-tools-and-action-registry.md), [Agent Control Loop](05-agent-control-loop.md), and the [event bus](02-event-bus-and-watchdogs.md); index at [index.md](index.md).

## File map

| File | Role |
| --- | --- |
| [`server.py`](../../browser_use/mcp/server.py) | `BrowserUseServer` — the 16-tool stdio server (1287 lines). |
| [`client.py`](../../browser_use/mcp/client.py) | `MCPClient` — canonical external-server client; JSON-Schema→pydantic action registration. |
| [`controller.py`](../../browser_use/mcp/controller.py) | `MCPToolWrapper` — older/simpler client variant (`register_mcp_tools` helper). |
| [`__main__.py`](../../browser_use/mcp/__main__.py) | `python -m browser_use.mcp` → `server.main()`. |
| [`__init__.py`](../../browser_use/mcp/__init__.py) | Exports `MCPClient`, `MCPToolWrapper`; lazy-imports `BrowserUseServer`. |
| [`manifest.json`](../../browser_use/mcp/manifest.json) | Anthropic DXT descriptor (Claude Desktop one-click install, `user_config` schema). |

Entry point: `browser-use --mcp` → [`cli.py`](../../browser_use/cli.py) `_run_mcp_server()` → `asyncio.run(server.main())`.

---

## Server mode

### Bootstrap & the shared-single-session model

`BrowserUseServer.__init__(session_timeout_minutes=10)` constructs a `mcp.server.Server('browser-use')`, loads `load_browser_use_config()`, and holds **one** of everything:

```python
self.agent: Agent | None
self.browser_session: BrowserSession | None   # the ONE interactive session
self.tools: Tools | None
self.llm: ChatOpenAI | None
self.file_system: FileSystem | None
self.active_sessions: dict[str, dict[str, Any]]   # session_id -> bookkeeping
```

All direct browser tools operate on the single `self.browser_session`. `_init_browser_session()` is idempotent (early-returns if a session exists) and is called lazily on the first `browser_*` tool. It builds a `BrowserProfile` from config layered over MCP defaults (`headless=False`, `keep_alive=True`, `wait_between_actions=0.5`, `user_data_dir=~/.config/browseruse/profiles/default`, downloads to `~/Downloads/browser-use-mcp`), starts the session, instantiates `Tools()`, and — only if an API key is present — a `ChatOpenAI` for the two LLM-backed tools plus a `FileSystem` for extraction.

**Gotcha:** the default model string in `_init_browser_session` is `'gpt-o4-mini'` (a typo), whereas `_retry_with_browser_use_agent` defaults to `'gpt-4o'`. The interactive `browser_extract_content` path therefore has a different (broken) default than agent delegation; both are normally overridden by config.

### The 16 tools

`handle_list_tools()` returns 16 `types.Tool` descriptors, each with a hand-written JSON-Schema `inputSchema`. Grouped by dispatch behaviour:

- **Bus-backed mutations** (dispatch a typed event, `await` it): `browser_navigate`, `browser_click`, `browser_type`, `browser_scroll`, `browser_go_back`, `browser_switch_tab`, `browser_close_tab`.
- **Direct reads** (call a `BrowserSession` helper, no bus): `browser_get_state`, `browser_get_html`, `browser_screenshot`, `browser_list_tabs`.
- **LLM-backed** (need the server's own `OPENAI_API_KEY`): `browser_extract_content`, `retry_with_browser_use_agent`.
- **Session management** (no active session required): `browser_list_sessions`, `browser_close_session`, `browser_close_all`.

That is 14 low-level primitives + 2 LLM-backed = 16. `browser_close` has a live dispatch branch in `_execute_tool` but is **commented out** of `handle_list_tools`, so it is unreachable over the wire. `handle_list_resources` and `handle_list_prompts` return empty lists (prompts in `manifest.json` are DXT metadata, not served MCP prompts).

### The `_execute_tool` dispatch ladder

`handle_call_tool` wraps `_execute_tool` with try/except (errors become `TextContent('Error: …')`) and a `finally` that emits an `MCPServerTelemetryEvent`. `_execute_tool(tool_name, arguments)` is a hand-written ladder, ordered so session-independent tools resolve before any session is forced into existence:

```
if  tool_name == 'retry_with_browser_use_agent'   -> agent delegation
if  tool_name == 'browser_list_sessions'          -> _list_sessions()
elif tool_name in {browser_close_session, browser_close_all}
elif tool_name.startswith('browser_'):
        if not self.browser_session: await self._init_browser_session()
        <inner if/elif on the exact tool name>
return f'Unknown tool: {tool_name}'
```

Return type is `str | list[TextContent | ImageContent]`. Most tools return a string; `browser_get_state` and `browser_screenshot` return a list so the base64 PNG rides as a separate `ImageContent` rather than being inlined into the JSON text (keeps the JSON small and lets the client render the image).

### RPC → bubus-event translation

The mutation handlers are the crux: each MCP call becomes exactly one event dispatched onto the session's control-plane bus, then awaited to completion. The uniform shape:

```python
from browser_use.browser.events import NavigateToUrlEvent
event = self.browser_session.event_bus.dispatch(NavigateToUrlEvent(url=url, new_tab=new_tab))
await event
return f'Navigated to: {url}'
```

Event mapping: `NavigateToUrlEvent` (navigate / new-tab / link-in-new-tab), `ClickElementEvent` + `ClickCoordinateEvent`, `TypeTextEvent`, `ScrollEvent(direction, amount=500)`, `GoBackEvent`, `SwitchTabEvent(target_id)`, `CloseTabEvent(target_id)`, `BrowserStopEvent`. Index-based tools first resolve the element via `get_dom_element_by_index(index)` — the `index` is the `backendNodeId` contract from the [DOM pipeline](03-dom-perception-pipeline.md)'s `selector_map`. Tab tools translate a 4-char `tab_id` (last 4 chars of the CDP `targetId`) back to a full target via `get_target_id_from_tab_id`.

Reads bypass the bus: `_get_html` runs raw `Runtime.evaluate` through `get_or_create_cdp_session`; `_screenshot` calls `take_screenshot`; `_get_browser_state` calls `get_browser_state_summary` and reshapes `selector_map` into a compact `interactive_elements` list with viewport/scroll metadata.

`_type_text` carries a conservative sensitivity heuristic: strings ≥6 chars matching an email or an ≥16-char mixed-alnum-with-punctuation pattern are dispatched with `is_sensitive=True` and a generic `sensitive_key_name` (`'email'` / `'credential'`), and the echoed confirmation is redacted to `Typed <credential> into element N`.

### Agent delegation (`retry_with_browser_use_agent`)

This is the escape hatch: hand a natural-language task to a full autonomous [Agent](05-agent-control-loop.md). It selects the LLM from config — `ChatAWSBedrock` when `model_provider == 'bedrock'`, otherwise `ChatOpenAI` honouring `base_url` — builds a fresh `BrowserProfile`, and runs `Agent(task, llm, browser_profile, use_vision).run(max_steps)`. Results are flattened to a text report (`steps`, `is_successful()`, `final_result()`, `errors()`, `urls()`), and `agent.close()` runs in `finally`.

**Gotcha — two disjoint session models.** Delegation creates its **own** `Agent`/`BrowserSession` and closes it afterward; it does **not** reuse `self.browser_session` and is **not** entered into `active_sessions`. So `browser_list_sessions` only ever shows the interactive session, and state does not carry between the delegated sub-agent and the direct `browser_*` tools.

**Gotcha — `allowed_domains=[]`.** An empty list is deliberately treated as "no override," because `SecurityWatchdog` interprets `allowed_domains=[]` as *unrestricted*; honouring an empty list would silently disable an admin-configured allowlist. Both `_retry_with_browser_use_agent` and `_init_browser_session` only override when the list is non-empty.

### Session lifecycle & cleanup

`_track_session` records `created_at`/`last_activity`/`url`; mutation handlers call `_update_session_activity`. `run()` launches a background `cleanup_loop` (via `create_task_with_error_handling`) that every 120 s closes sessions idle beyond `session_timeout_minutes`. Since only the shared session is tracked and refreshed, this is effectively an idle-timeout for the single interactive browser.

### stdout-purity logging surgery

MCP stdio framing **owns stdout** — one stray log line or `print` corrupts the JSON-RPC stream and kills the session. `server.py` therefore performs unusually aggressive logging containment, layered defensively because import-time side effects can create loggers before you get a chance to muzzle them:

1. **Before any `browser_use` import**, set `BROWSER_USE_LOGGING_LEVEL='critical'` and `BROWSER_USE_SETUP_LOGGING='false'` (module top, lines 30–31).
2. `logging.basicConfig(stream=sys.stderr, force=True)` and `_configure_mcp_server_logging()` — re-point root and every existing logger at a single **stderr** handler, level `CRITICAL`, `propagate=False`.
3. `logging.disable(logging.CRITICAL)` — global kill switch.
4. `_ensure_all_loggers_use_stderr()` — re-swept after imports, again in `__init__`, and again before browser init, because new loggers keep appearing. It walks `logging.root.manager.loggerDict` and forces every logger onto the shared stderr handler.
5. The MCP SDK's own `mcp` logger is separately pinned to stderr at `ERROR`.

`run()` opens `mcp.server.stdio.stdio_server()` and calls `server.run(read, write, InitializationOptions(...))`; a `BrokenPipeError` (client vanished mid-write) is caught and treated as a clean shutdown. `main()` brackets the run with `start`/`stop` telemetry and a `flush()`.

---

## Client mode

The mirror image: discover an external server's tools and register each as a native browser-use action so the Agent can invoke them exactly like `click`/`type`.

### `MCPClient` ([`client.py`](../../browser_use/mcp/client.py))

```python
MCPClient(server_name, command, args=None, env=None)
await client.connect()
await client.register_to_tools(tools, tool_filter=None, prefix=None)
# ... run Agent ...
await client.disconnect()          # or use `async with MCPClient(...) as c:`
```

**Connection.** `connect()` spawns `_run_stdio_client` as a background task, then polls `self._connected` for up to 100×0.1 s (10 s) before raising. `_run_stdio_client` enters `stdio_client(server_params)` → `ClientSession`, `await session.initialize()`, `session.list_tools()` into `self._tools`, sets `_connected=True`, then blocks on `await self._disconnect_event.wait()`. This background-task-plus-event structure exists because `stdio_client` is an async context manager with anyio task-scoping: it must be entered and exited in the *same* task, so the connection is opened in a dedicated task and torn down by signalling the event (not by exiting the context elsewhere). `disconnect()` sets the event, `await asyncio.wait_for(task, 2.0)`, cancels on timeout.

### JSON-Schema → dynamic pydantic model → action

`register_to_tools` iterates discovered tools (honouring `tool_filter` and an optional `prefix`), skips duplicates, and calls `_register_tool_as_action(registry, action_name, tool)`. That function is the interesting bit — it manufactures a pydantic model from the tool's JSON-Schema at runtime:

- For each `inputSchema.properties[name]`: `_json_schema_to_python_type(schema)` → a Python type. Required names get default `...`; optional names become `type | None` with the schema `default` (or `None`).
- `param_model = create_model(f'{action_name}_Params', __base__=ConfiguredBaseModel, **param_fields)` where `ConfiguredBaseModel` carries `model_config = ConfigDict(extra='forbid', validate_by_name=True, validate_by_alias=True)`. No params → `param_model = None`.
- `_json_schema_to_python_type` recurses: `enum`→`str`; `object` **with** `properties`→a nested `create_model`; `object` without→`dict`; `array` with `items`→`list[item_type]`, else `list`; `nullable`/`null`→`base_type | None`. Primitive map is `string→str, number→float, integer→int, boolean→bool`.

Two wrapper shapes are generated so the Registry's signature introspection is satisfied — one is defined as `async def mcp_action_wrapper(params: param_model) -> ActionResult`, the other as a zero-arg `async def` when there are no parameters (the annotation, not `**kwargs`, is what the Registry reads to bind the action's param model). The wrapper `model_dump(exclude_none=True)` → `session.call_tool(tool.name, tool_params)` → `_format_mcp_result` → `ActionResult`. Registration is the final line:

```python
registry.action(description=description, param_model=param_model, domains=domains)(mcp_action_wrapper)
```

`_format_mcp_result` normalises the SDK's response (`result.content` list/scalar, bare list, or `str`), pulling `.text` off each content item and `\n`-joining. On success the action returns `ActionResult(extracted_content=…, long_term_memory=f"Used MCP tool '{tool.name}' from {self.server_name}", include_extracted_content_only_once=True)`; on failure `ActionResult(error=…, success=False)`. `is_browser_tool`/`domains` are computed but currently inert (`domains` stays `None`) — a vestige of the removed `page_filter`.

### `MCPToolWrapper` ([`controller.py`](../../browser_use/mcp/controller.py))

The older sibling, reachable via `register_mcp_tools(registry, mcp_command, mcp_args)`. Same JSON-Schema→pydantic idea, but: a flat type map (no recursion into nested objects/arrays), a `**kwargs` wrapper that strips injected specials (`browser_session`, `page_extraction_llm`, `file_system`, …) before calling the tool, and — critically — `connect()` opens `stdio_client`/`ClientSession` **inline** and then `await self._keep_session_alive()`, so `connect()` never returns until shutdown. Its own docstring flags this ("you'd want to manage this lifecycle better"). Prefer `MCPClient` for new code.

### Invariants

- Registered action names must be unique within a `Registry`; `prefix` is the collision-avoidance knob when composing several servers.
- A tool call only works while `self.session and self._connected`; the wrappers short-circuit to an error `ActionResult` otherwise.
- Discovery is one-shot at connect time — `self._tools` is not refreshed if the server's tool list changes mid-session.

---

## Rust port notes

**Maps cleanly.**
- *Server transport & dispatch.* The stdio JSON-RPC server maps onto the [`rmcp`](https://crates.io/crates/rmcp) crate (or a hand-rolled newline-framed JSON-RPC 2.0 loop — which the [beta Rust bridge](11-beta-rust-bridge.md) already implements and is the authoritative framing spec). The `_execute_tool` ladder becomes a `match tool_name`.
- *Logging surgery evaporates.* Python's global mutable logger hierarchy is why `server.py` re-sweeps loggers four times. In Rust, a single `tracing_subscriber` `fmt` layer pinned to `io::stderr` guarantees stdout purity structurally — there is no ambient root logger for late-created modules to leak through.
- *Bus translation.* Each handler's "dispatch typed event, await it" is a direct `bus.dispatch(Event).await` once the [event bus](02-event-bus-and-watchdogs.md) is ported.
- *Shared-single-session model.* `Option<BrowserSession>` behind an `Arc<Mutex<…>>`, or better an actor task owning the session with an mpsc command channel.

**Needs redesign.**
- *Dynamic pydantic models.* `create_model` has no Rust analog — Rust has no runtime struct synthesis. Do **not** try to generate types. Keep tool params as `serde_json::Value`, store the JSON-Schema alongside a `Box<dyn Fn(Value) -> BoxFuture<ActionResult>>` closure in the registry, and validate at call time with the [`jsonschema`](https://crates.io/crates/jsonschema) crate. The Registry's Python trick of reading a function's first-param annotation to bind a param model is a language-specific hack that simply disappears.
- *anyio task-scoping dance.* `MCPClient`'s background-task-plus-`_disconnect_event` pattern is a workaround for Python async-context-manager scoping. With `tokio::process::Child` + framed stdio you own the child handle directly; the connection is a struct field dropped on `disconnect`, no keep-alive task required.
- *Agent delegation* pulls in the whole Agent/LLM stack ([05](05-agent-control-loop.md)/[06](06-llm-provider-abstraction.md)); it ports only after those layers exist. The provider fork (`ChatAWSBedrock` vs `ChatOpenAI`) becomes an enum dispatch behind the `BaseChatModel` trait object.
