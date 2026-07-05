# The Beta Rust Bridge & JSON-RPC Contract

`browser_use.beta` is a drop-in `Agent` that offloads the entire perceive→decide→act loop to a
pre-existing native binary (`browser-use-terminal`), driving it over newline-framed JSON-RPC 2.0
on stdio and reconstructing an `AgentHistoryList` from the event stream it emits. Everything in this
file lives in [`browser_use/beta/service.py`](../../browser_use/beta/service.py) (~6800 LOC) with a
lazy re-export shim in [`browser_use/beta/__init__.py`](../../browser_use/beta/__init__.py). This
doc treats the wire protocol as the authoritative spec any future Rust core must honor — it is the
seam between today's Python orchestrator (layers 1–8, see [index.md](index.md)) and the Rust rewrite.

## Position in the system

The beta `Agent` is API-compatible with [`browser_use/agent/service.py`](../../browser_use/agent/service.py)
(it even rewrites `Agent.__module__`/`__signature__` to mimic `_PythonAgent` via
`_align_browser_use_agent_signatures()` at the bottom of the file), but it owns **none** of the
in-process machinery this doc-set otherwise describes. The CDP transport
([01](01-cdp-transport-and-session-manager.md)), event bus + watchdogs
([02](02-event-bus-and-watchdogs.md)), DOM pipeline ([03](03-dom-perception-pipeline.md)), tools
([04](04-tools-and-action-registry.md)) and the step loop ([05](05-agent-control-loop.md)) all run
**inside the Rust process**. The Python side is a thin RPC client + config translator + history
reconstructor. When a `cdp_url` is supplied, the Rust process attaches to a Python-launched browser
(`browser_mode='remote-cdp'`); otherwise the Rust process manages its own Chromium.

## Binary discovery

`find_browser_use_terminal_binary() -> str` resolves the executable in priority order:

1. `$BROWSER_USE_TERMINAL_BINARY` (explicit override, no capability check).
2. `_find_packaged_browser_use_terminal_binary()` — imports `browser_use_core.binary_path('browser-use-terminal')`.
3. `$BUT_HOME/packages/standalone/current/bin/browser-use-terminal` (default `~/.browser-use-terminal`)
   then `$BUT_INSTALL_DIR/browser-use-terminal` (default `~/.local/bin`).
4. `shutil.which('browser-use-terminal')` on `PATH`.

Candidates 3–4 are gated by `_terminal_supports_sdk_server(binary)`, which runs `binary --help` (5 s
timeout) and greps stdout+stderr for the substring `sdk-server`. Missing/incapable → `BetaAgentError`
pointing at `curl -fsSL https://browser-use.com/terminal/install.sh | sh`.

`_apply_agent_tools_env()` additionally locates a sibling `agent-tools/` dir containing `rg`
(`rg.exe` on Windows) and prepends it to `PATH` under `$BUT_AGENT_TOOLS_DIR`, so the Rust side's
shell tools can find ripgrep.

## Transport: `RustSdkClient`

`RustSdkClient(command, env)` is a minimal stdio JSON-RPC 2.0 client. `_sdk_server_argv()` builds the
command as `[<binary>, *_state_dir_args(), 'sdk-server', '--transport', 'stdio']`
(`$BROWSER_USE_SDK_SERVER` overrides the binary; `$BROWSER_USE_RUST_STATE_DIR` → `--state-dir`).

- **Framing** — one JSON object per line. Requests are `json.dumps(request) + '\n'` written to stdin
  under an `asyncio.Lock` (`_write_lock`); responses/notifications are parsed line-by-line from
  stdout in `_read_stdout()`, which manually scans for `b'\n'` and decodes UTF-8 with
  `errors='replace'`. Empty lines are skipped; a line that fails `json.loads` fails **all** pending
  futures (`_fail_all`) — the stream is fail-fast, not resync-on-error.
- **Buffers** — stream limits are tunable via env: `BROWSER_USE_SDK_STREAM_LIMIT_BYTES` (default
  64 MiB, the `create_subprocess_exec(limit=...)`), `BROWSER_USE_SDK_READ_CHUNK_BYTES` (1 MiB reads),
  `BROWSER_USE_SDK_MAX_LINE_BYTES` (512 MiB max unframed buffer before erroring). These large caps
  exist because a full `history` payload with base64 screenshots can be enormous.
- **Request IDs** — monotonic `int` from `_next_id` (starts at 1). `id` **must** be an integer in
  responses (`_handle_message` rejects non-int ids). Each id maps to an `asyncio.Future` in
  `_pending`; `call(method, params)` awaits it.
- **stderr** — drained concurrently into a ring buffer (`stderr_lines`, last 500). On a stdout reader
  failure or premature exit with pending calls, the last 20 stderr lines are appended to the error.

`call()` auto-starts the process (idempotent `start()`), writes the frame, and awaits. `close()`
closes stdin, `terminate()`s with a 2 s grace then `kill()`, cancels reader tasks, and fails all
pending futures with `BetaAgentError('Rust SDK server closed')`.

## Handshake: `runtime.ping`

`_ensure_sdk_client()` reuses a live client or spawns a fresh one, then immediately calls
`runtime.ping` (no params). The result **must** be `{"sdk_protocol_version": 1}`; anything else
closes the client and raises `BetaAgentError`. This is the sole version negotiation — protocol
version 1 is hard-coded. Reconnecting after a dead process resets `_sdk_agent_id`, `_sdk_browser_id`,
and `terminal_session_id` to `None`.

## RPC method surface

| Method | Direction | When | Params (key subset) | Result |
| --- | --- | --- | --- | --- |
| `runtime.ping` | req/resp | on connect | — | `{sdk_protocol_version: 1}` |
| `agent.run_task` | req/resp | first run of a session | full run params (below) | `{agent_id, session_id, browser_id, history}` |
| `agent.run` | req/resp | follow-ups / when `agent_id` exists | run params + `agent_id`, `browser_id`, `followups` | same |
| `agent.close` | req/resp | teardown (non keep-alive) | `{agent_id}` | — |
| `browser.close` | req/resp | teardown (non keep-alive) | `{browser_id}` | — |
| `agent.event` | notification | during run | `{event: {...}}` | (no id) |
| `agent.projected_event` | notification | during run | `{event: {...}}` | (no id) |

Method selection in `_run_sdk_agent()`:
`method = 'agent.run' if self._sdk_agent_id or followups else 'agent.run_task'`. The result dict's
`agent_id`/`session_id`/`browser_id` are cached onto the Agent so subsequent calls resume the same
Rust-side session (`terminal_session_id` mirrors `session_id`).

### Run params — `_sdk_run_params()`

```python
{
  'task': str,                       # possibly rewritten with initial-nav/schema context
  'cwd': os.getcwd(),
  'llm': {'provider', 'model', 'timeout'?},   # _sdk_llm_payload()
  'max_steps': int,
  'browser_mode': str,               # _browser_mode()
  'browser': {...},                  # _sdk_browser_payload()
  'calculate_cost': bool,
  'use_vision': bool,
  'max_actions_per_step': int,
  'config_overrides': {'full_llm_input_events': True},
  'agent_id'?: str, 'browser_id'?: str, 'followups'?: [str], 'output_schema'?: dict,
}
```

`config_overrides.full_llm_input_events=True` requests that the Rust side emit the full LLM input
item events needed for faithful history/usage reconstruction — without it the reconstructor loses
`model.response.input_item` detail. `output_schema` carries the structured-output JSON schema
(`extraction_schema`, defaulted from `output_model_schema.model_json_schema()`).

### `browser` payload — `_sdk_browser_payload()`

A sparse dict (keys omitted when `None`/empty via the local `put()`): `cdp_url`, `cdp_headers`,
`user_agent`, `viewport`, `window_size`, `storage_state`, `downloads_path`, `allowed_domains`,
`blocked_domains`, `state_dir`, `no_viewport`, `accept_downloads`, `headless`, `keep_alive`,
`profile_id`, `proxy_country_code`. These are extracted from the `BrowserSession`/`BrowserProfile`
by the many `_extract_*`/`_managed_*` free functions (session profile wins over passed profile).

## `browser_mode` resolution — `_browser_mode()`

Precedence: a `cdp_url` on session or profile → `'remote-cdp'`; else `$BROWSER_USE_RUST_BROWSER_MODE`;
else `$BROWSER_USE_BROWSER_MODE`; else a cloud preference (`use_cloud`/`cloud_browser`) → `'cloud'`;
else `headless is False` → `'managed-headed'`, otherwise `'managed-headless'` (the default).
`_is_managed_browser_mode()` normalizes hyphen/underscore/space variants and gates whether
`BU_MANAGED_BROWSER_*` env and `CHROME_PATH` are forwarded.

## Config translation via environment — `_run_env()`

Beyond the structured `browser` payload, a large surface is passed through **environment variables**
on the spawned process (`os.environ.copy()` + overrides). Two families:

**`LLM_BROWSER_*` (provider credentials)** — `_llm_env_overrides(llm)` maps the resolved provider to
env: OpenAI → `LLM_BROWSER_OPENAI_API_KEY`/`_BASE_URL`; Anthropic → `LLM_BROWSER_ANTHROPIC_*`;
OpenRouter → `OPENROUTER_API_KEY`/`_BASE_URL`; DeepSeek → `DEEPSEEK_API_KEY`; browser-use →
`LLM_BROWSER_BROWSER_USE_*` (base URL stripped of a trailing `/v1`). `LLM_BROWSER_BROWSER_MODE` is
always set to the resolved `browser_mode`. Cost accounting sets `BU_USE_CALCULATE_COST=true` and, for
OpenAI-compatible providers, `LLM_BROWSER_OPENAI_COMPAT_INCLUDE_USAGE=true`.

**`BU_BROWSER_*` (browser behavior)** — `BU_CDP_URL`, `BU_CDP_HEADERS` (JSON), `BU_BROWSER_USER_AGENT`,
`BU_BROWSER_BLOCK_IP_ADDRESSES`, `BU_BROWSER_ALLOWED_DOMAINS`/`_PROHIBITED_DOMAINS` (JSON arrays),
`BU_BROWSER_PERMISSIONS` (JSON), `BU_BROWSER_ACCEPT_DOWNLOADS`, `BU_BROWSER_DOWNLOADS_PATH`,
`BU_BROWSER_NO_VIEWPORT`, `BU_BROWSER_VIEWPORT` (JSON), `BU_BROWSER_STORAGE_STATE` (JSON), plus the
wait-timing trio built by `_extract_wait_timing_settings()`:
`BU_BROWSER_MINIMUM_WAIT_PAGE_LOAD_MS`, `BU_BROWSER_NETWORK_IDLE_PAGE_LOAD_MS`,
`BU_BROWSER_WAIT_BETWEEN_ACTIONS_MS` (seconds→ms). Highlight settings map to
`BROWSER_USE_TERMINAL_AUTO_HIGHLIGHT`/`_HIGHLIGHT_COLOR`/`_HIGHLIGHT_DURATION_MS`. In managed modes
only: `BU_MANAGED_BROWSER_ARGS` (JSON chrome flags from `_managed_browser_launch_args()`),
`BU_MANAGED_BROWSER_PROFILE`, and `CHROME_PATH`.

Note the **dual channel**: many browser settings travel both in the `browser` JSON payload *and* as
`BU_BROWSER_*` env. A native core must decide precedence; the current Rust binary honors both.

## Notification stream — `agent.event` / `agent.projected_event`

Notifications (JSON-RPC messages with no `id`) whose `method` is `agent.event` or
`agent.projected_event` are appended to `sdk.notifications` (ring-buffered to 2000) and pushed onto an
`asyncio.Queue` (`notification_queue`). `_log_sdk_progress()` drains the queue purely for human-facing
progress logging (dedup + 30 s throttle via `_sdk_notification_summary()`, which suppresses
`*.output_delta` streaming events). All other message shapes are treated as responses.

**Envelope.** Each notification is `{"method": ..., "params": {"event": <event>}}`. The canonical
`<event>` is `{seq, id, session_id, ts_ms, event_type, payload}`. `agent.projected_event` and some
double-wrapped `agent.event` forms nest the real event under `payload.event_type`/`payload.payload`;
`_sdk_notification_events()` unwraps both, normalizes to the canonical shape, dedupes on
`(seq, id, event_type)` (falling back to `(index, method, event_type)`), and finally runs
`_dedupe_sdk_events()`.

## Event → `AgentHistoryList` reconstruction

The `history` in the RPC **result** is authoritative when present:
`result['history'] = {events, child_events, usage_events, usage, success, errors}`. The streamed
notification events are a **fallback / supplement**. `_run_sdk_agent()` prefers the response events
but swaps in notification events when the response is empty, transport-truncated
(`sdk.transport.truncated`), shorter than the notification set, or lacks a final result while the
notifications have one (`_result_from_events`). This dual-path design tolerates a Rust process that
dies mid-write after already streaming a valid `session.done`.

`_history_from_events(events, *, model, started, finished, output_model_schema, process_error)` is the
core reconstructor:

1. **Replay semantics** — `_events_after_terminal_compaction()` honors `session.compacted`
   (`replay_from_seq`) and `_events_after_terminal_rollbacks()` unwinds `session.rollback` by
   deleting the last N terminal user turns. This makes the reconstruction a faithful replay of the
   Rust session's *logical* history, not just its raw log.
2. **Final result** — `_result_from_events()` looks for a `done` tool call, then `session.done`
   (`result` or `result_file`), then `agent.completed`. Structured output is coerced against
   `output_model_schema`.
3. **Failure** — `process_error` (transport/exception) or `_failure_from_events()`
   (`session.failed`/`agent.failed`/`tool.failed`/…). `is_done = final_result is not None and failure is None`.
4. **Step segmentation** — `_terminal_turn_spans()` splits on `model.turn.request` boundaries (each
   LLM turn = one `AgentHistory` step). `_history_items_from_terminal_turns()` builds per-step
   `AgentHistory(model_output, result, state, metadata)` from tool events; if no spans exist it
   collapses to a single history item.
5. **Usage** — `_usage_from_events()` folds `model.usage` and `token_count` events into a
   `UsageSummary`/`ModelUsageStats`; `_usage_event_from_sdk_history_usage()` promotes the result's
   aggregate `usage` when it exceeds the per-event sum.

Transport errors that arrive *after* a valid final result (`CancelledError`, JSON-RPC line-overflow,
reader failure — see `_sdk_transport_error_after_final_result()`) are cleared so a truncated-but-done
run still reports success. `_pending_history_prefix` (initial-action steps run before the terminal
handoff) is prepended.

`_load_rust_history(file_path)` is a separate path that validates a serialized `AgentHistoryList`
JSON file directly (nulling `model_output`, defaulting `interacted_element`).

### Event vocabulary (the contract a Rust core must emit)

`event_type` strings consumed by the reconstructor: `model.turn.request` (step delimiter),
`model.turn.error`, `model.usage`, `model.response.input_item`, `model.response.output_item`,
`tool.started`/`tool.output`/`tool.image`/`tool.finished`/`tool.failed`/`tool.aborted`,
`token_count`, `session.input`, `session.done`, `session.failed`, `session.cancelled`,
`session.interrupted`, `session.compacted`, `session.rollback`,
`session.final_answer_not_ready_at_max_turns`, `agent.message`, `agent.failed`, `agent.cancelled`,
`agent.completed`, `browser.cleanup_timed_out`, `browser_script`/`browser_script.failed`,
`exec_command.end`, `sdk.transport.truncated`, plus legacy aliases (`tool_result`, `browser_script`).
`_event_type()` reads `event_type` or `type`; `_event_payload()` reads `payload`; `_event_seq()`
reads `seq`/`event_seq`/`sequence`; `ts_ms` (ms epoch) drives step timestamps.

## Session lifecycle & invariants

- **Session resumption**: `run()` first-time → `agent.run_task`; `follow_up(task)` requires an
  existing `terminal_session_id` + `_sdk_agent_id` and issues `agent.run` with `followups=[task]`.
- **Teardown**: `close()` → `_close_sdk_browser_resources()` sends `agent.close`+`browser.close`
  (suppressing errors) then closes the client — **unless** `keep_alive` is set on the profile, in
  which case the Rust process and its browser survive for reuse.
- **Cancellation**: SIGINT/`stop()` schedules `_cancel_active_sdk_run()` (which just `close()`s the
  client, killing the process); `_preserve_sdk_notification_history()` salvages whatever events
  streamed before the kill so `self.history` is still populated.
- **Invariants**: response `id` is always `int`; protocol version is always `1`; exactly one JSON
  object per stdout line; a malformed line is unrecoverable (fails all pending). `browser_mode`
  forced to `remote-cdp` whenever a `cdp_url` exists, regardless of env overrides.

## Gotchas

- **Silent env precedence** — browser config is duplicated across the `browser` payload and
  `BU_BROWSER_*` env; changing one without the other creates drift.
- **512 MiB line ceiling** — a single history frame with many base64 screenshots can approach the
  `BROWSER_USE_SDK_MAX_LINE_BYTES` cap; exceeding it fails the whole run. The notification fallback
  exists partly to recover such cases.
- **Model name fallback** — `_model_name()` reads `llm.model`/`model_name`/`name`, else
  `$BROWSER_USE_RUST_MODEL` (default `gpt-5.3-codex-spark`). A missing/odd LLM object silently yields
  that default rather than erroring.
- **Dedup is heuristic** — projected vs. response duplicates are merged on `(seq, id, event_type)` +
  a payload fingerprint; events lacking `seq`/`id` fall back to positional identity, so a Rust core
  that omits `seq` will get order-sensitive dedup.

## Rust port notes

- **This *is* the target seam.** A native Rust core replaces the `sdk-server` binary; the Python
  `RustSdkClient` becomes optional (the loop can run in-process). The contract above — `runtime.ping`
  → `agent.run_task`/`agent.run` → the `agent.event` stream + result `history` — is the ABI to keep
  stable during the migration so the Python shim keeps working unchanged.
- **Maps cleanly**: newline-framed JSON-RPC 2.0 over stdio is trivial in Rust
  (`tokio::process` + `LinesCodec`/`tokio_util`, `serde_json` for framing, `jsonrpsee`-style id/future
  maps). The `event_type`/`payload` envelope is a plain tagged union — model it as
  `#[serde(tag = "event_type")]` enums with `serde(other)` for forward-compat.
- **Needs care**: the reconstruction is a *replay VM* (compaction `replay_from_seq`, rollback of the
  last N user turns, projected-vs-response dedup). If the Rust core emits history directly this logic
  can be dropped, but the two-channel fallback (response `history` vs. streamed notifications) only
  matters when transport can truncate — an in-process core removes that failure mode entirely, so
  most of `_dedupe_sdk_events`/`_sdk_notification_events`/`_sdk_transport_error_after_final_result`
  has no analogue. Keep the *event schema* and the `AgentHistoryList` mapping; discard the transport
  salvage machinery.
- **Config translation** (`_run_env`, `_sdk_browser_payload`, `_browser_mode`) is the least glamorous
  but highest-fidelity-risk surface: the dual env/payload channel and the `_extract_*` precedence
  ladder (session profile → passed profile → session object) must be reproduced exactly or profiles
  silently regress.

See [09-configuration-logging-bootstrap.md](09-configuration-logging-bootstrap.md) for the shared
`CONFIG`/env conventions and [05-agent-control-loop.md](05-agent-control-loop.md) for the in-process
loop this subsystem replaces.
