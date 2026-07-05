# Agent Orchestrator & LLM Action Loop

The `Agent` ([`browser_use/agent/service.py`](../../browser_use/agent/service.py)) is the top layer: it owns the task, the LLM, the `BrowserSession`, the `Tools` registry, and a `MessageManager`, and drives a bounded **perceive → decide → act** loop. Each iteration renders the browser state into a prompt, asks the LLM for a dynamically-typed `AgentOutput` (reasoning + a list of actions), executes those actions with mid-sequence guards, and records a history item — until an action calls `done` or the step budget runs out.

This doc covers the layer-7 orchestrator. For what actions *do* see [`04-tools-and-action-registry.md`](04-tools-and-action-registry.md); for how `AgentOutput` gets turned into wire JSON see [`06-llm-provider-abstraction.md`](06-llm-provider-abstraction.md); for the state the loop perceives see [`03-dom-perception-pipeline.md`](03-dom-perception-pipeline.md). Index: [`index.md`](index.md).

## Key types

| Type | File | Role |
| --- | --- | --- |
| `Agent[Context, AgentStructuredOutput]` | [`service.py`](../../browser_use/agent/service.py) | Orchestrator; generic over an optional user context object and a pydantic structured-output schema. |
| `AgentSettings` | [`views.py`](../../browser_use/agent/views.py) | All tunables (`use_vision`, `use_thinking`, `flash_mode`, `max_failures`, `max_actions_per_step`, timeouts, planning, loop detection, compaction). |
| `AgentState` | [`views.py`](../../browser_use/agent/views.py) | Mutable run state: `n_steps` (starts at **1**), `consecutive_failures`, `last_model_output`, `last_result`, `plan`, `paused`/`stopped`, `loop_detector`, `message_manager_state`. Serializable for checkpointing. |
| `AgentOutput` | [`views.py`](../../browser_use/agent/views.py) | The structured LLM response — `thinking`, `evaluation_previous_goal`, `memory`, `next_goal`, `current_plan_item`, `plan_update`, and `action: list[ActionModel]`. |
| `AgentStepInfo` | [`views.py`](../../browser_use/agent/views.py) | `(step_number, max_steps)` dataclass; `is_last_step()`. |
| `MessageManager` | [`message_manager/service.py`](../../browser_use/agent/message_manager/service.py) | Builds the 3-slot prompt from history + browser state. |
| `AgentHistoryList` | [`views.py`](../../browser_use/agent/views.py) | Ordered `AgentHistory` items; `final_result()`, `is_done()`, `structured_output()`, `usage`. |

## The 3-phase `step()` pipeline

`Agent.step(step_info)` ([`service.py:1027`](../../browser_use/agent/service.py)) is the heartbeat. It is wrapped in one `try/except/finally` so that **every** exception funnels through `_handle_step_error` and **every** exit runs `_finalize`:

- **Phase 0 — captcha wait** (pre-phase): `browser_session.wait_if_captcha_solving()`. If it waited, the step clock is reset and the outcome is injected as an `ActionResult(long_term_memory=...)` so the LLM sees it. Non-fatal on error.
- **Phase 1 — `_prepare_context()`** ([`service.py:1079`](../../browser_use/agent/service.py)): the perceive step. Calls `browser_session.get_browser_state_summary(include_screenshot=True)` (screenshot is *always* captured, even with `use_vision=False`, because it's cheap and useful for cloud sync), checks downloads, refreshes page-scoped action models (`_update_action_models_for_page`), then hands everything to the `MessageManager` (`prepare_step_state` → `_maybe_compact_messages` → `create_state_messages`). Finally it appends a cascade of context nudges (budget, replan, exploration, loop, force-done). Returns the `BrowserStateSummary`.
- Between phases 1 and 2, `last_model_output` and `last_result` are **cleared** — after context is built (which needs the prior result for the "previous action result" prompt) but before the LLM call, so a timeout mid-decision can't leave stale data attributed to this step.
- **Phase 2 — decide + act**: `_get_next_action()` then `_execute_actions()`.
- **Phase 3 — `_post_process()`** ([`service.py:1211`](../../browser_use/agent/service.py)): download check, plan advance, loop-detector action recording, failure accounting, completion logging.
- **`finally` — `_finalize()`** ([`service.py:1348`](../../browser_use/agent/service.py)): builds and stores the `AgentHistory` item (via `_make_history_item`, which persists the screenshot through `screenshot_service`), saves filesystem state, dispatches `CreateAgentStepEvent` on the event bus, and **increments `n_steps`**.

`step()` itself never returns a value or raises to the caller — done-ness is discovered later by inspecting `self.history`.

### Phase 2 detail: decide

`_get_next_action()` ([`service.py:1168`](../../browser_use/agent/service.py)) pulls `input_messages = message_manager.get_messages()` and calls `_get_model_output_with_retry` under `asyncio.wait_for(timeout=settings.llm_timeout)` (default 60 s; auto-detected as 30 s for Gemini, 90 s for o3). Two `_check_stop_or_pause()` gates bracket the call so a Ctrl-C is honored before the output is committed. `_get_model_output_with_retry` ([`service.py:1662`](../../browser_use/agent/service.py)) retries **once** on an empty action list with a clarification message, then falls back to injecting a synthetic `done(success=False, text="No next action returned by LLM!")` so the loop never stalls on a malformed response.

### Phase 2 detail: act

`_execute_actions()` calls `multi_act(last_model_output.action)` (see below) and stores the list into `last_result`.

## `run()` lifecycle & `SignalHandler`

`run(max_steps=500, on_step_start, on_step_end)` ([`service.py:2492`](../../browser_use/agent/service.py)) is the public entry (`run_sync` wraps it for non-async callers). Order of operations:

1. Install a `SignalHandler` ([`browser_use/utils.py:118`](../../browser_use/utils.py)) wired to `pause`/`resume` and a `custom_exit_callback` that flushes telemetry. Disabled via `enable_signal_handler=False`.
2. Dispatch `CreateAgentSessionEvent` (once) and `CreateAgentTaskEvent`; `browser_session.start()`; register skills as actions; run `_execute_initial_actions()` under a `step_timeout` guard so a silent CDP socket on the opening navigate can't hang the whole run.
3. Main loop: `while self.state.n_steps <= max_steps`. Each iteration checks pause (`await _external_pause_event.wait()`), the failure ceiling (`max_failures + int(final_response_after_failure)`), and the `stopped` flag, then calls `_execute_step`, which runs `step()` under `asyncio.wait_for(timeout=settings.step_timeout)` (default 180 s). A step timeout is caught, counted as a failure, and manually advances `n_steps` so the loop can't wedge.
4. `_execute_step` returns `is_done` by consulting `history.is_done()`. On done it runs `log_completion`, the optional judge (`use_judge`), and `register_done_callback`, then breaks. The `while...else` clause fires only if the loop exhausts `max_steps` without done, appending a synthetic error history item.
5. `finally`: log token usage, unregister the signal handler, emit `UpdateAgentTaskEvent`, optionally render the GIF, and `await eventbus.stop(...)` + `close()`.

`SignalHandler` implements a two-stage Ctrl-C: the **first** SIGINT cancels interruptible tasks (matched by name pattern) and sets `paused`; the **second** SIGINT calls `_handle_second_ctrl_c` → `os._exit(0)` after resetting terminal modes. `_check_stop_or_pause()` ([`service.py:1005`](../../browser_use/agent/service.py)) is the cooperative cancellation point — it raises `InterruptedError` on `stopped`/`paused` or when `register_should_stop_callback` returns true; `_handle_step_error` treats `InterruptedError` as a benign warning, not a failure.

`take_step(step_info)` ([`service.py:2245`](../../browser_use/agent/service.py)) exposes a single external iteration returning `(is_done, is_valid)` for callers that want to drive the loop themselves.

## Dynamic `AgentOutput` (`type_with_custom_actions`)

The action vocabulary is not static — it's compiled into the response schema per run and per page. `_setup_action_models()` ([`service.py:772`](../../browser_use/agent/service.py)) asks the registry for an `ActionModel` (a `RootModel`-union over every registered action, see layer 6) and then builds the matching `AgentOutput` subclass:

- `flash_mode` → `type_with_custom_actions_flash_mode` — schema keeps only `memory` + `action` (drops `thinking`, `evaluation_previous_goal`, `next_goal`, plan fields). Cheapest, fastest.
- `use_thinking=True` → `type_with_custom_actions` — full schema including `thinking`.
- else → `type_with_custom_actions_no_thinking` — drops `thinking`, keeps the eval/memory/next-goal triad.

Each variant is a `pydantic.create_model(..., action=(list[custom_actions], Field(min_items=1)))` and overrides `model_json_schema` to force the `required` list ([`views.py:419-486`](../../browser_use/agent/views.py)). A parallel `DoneAgentOutput` restricted to the single `done` action is prebuilt; the loop hot-swaps `self.AgentOutput = self.DoneAgentOutput` when it wants to *force* completion (last step, or max-failures recovery). `_update_action_models_for_page(url)` ([`service.py:4004`](../../browser_use/agent/service.py)) rebuilds both models every step so URL-filtered actions appear/disappear as the browser navigates.

`get_model_output()` ([`service.py:1937`](../../browser_use/agent/service.py)) calls `llm.ainvoke(messages, output_format=self.AgentOutput, session_id=...)`, truncates `parsed.action` to `max_actions_per_step`, and broadcasts/logs. It also runs a URL-shortening round-trip: long URLs in the prompt are replaced with short tokens before the call and restored inside the parsed pydantic tree afterward (`_process_messsages_and_replace_long_urls_shorter_ones` / `_recursive_process_all_strings_inside_pydantic_model`) to save tokens.

## The 3-slot `MessageManager` & history compaction

`MessageManager` holds a `MessageHistory` ([`message_manager/views.py`](../../browser_use/agent/message_manager/views.py)) with exactly **three slots**, always emitted in order by `get_messages()`:

1. `system_message` — the system prompt (set once).
2. `state_message` — the single big user message rebuilt every step (task + agent history + browser state + screenshot + read-state).
3. `context_messages: list` — ephemeral per-step nudges (budget/replan/loop/last-step), cleared at the top of each `prepare_step_state`.

This is a deliberately **flat** context: rather than an ever-growing chat transcript, the entire run is re-serialized into one state message each step. The history itself lives as a list of frozen `HistoryItem`s (`step_number`, `evaluation_previous_goal`, `memory`, `next_goal`, `action_results`, or `error`/`system_message`) in `MessageManagerState.agent_history_items`; `agent_history_description` renders them, honoring `max_history_items` by keeping the first item + a `[... N previous steps omitted...]` marker + the most recent items.

`create_state_messages()` decides screenshot inclusion from `use_vision` (`True` = always, `'auto'` = only if an `ActionResult.metadata['include_screenshot']` requested it, `False` = never), then delegates rendering to `AgentMessagePrompt` ([`prompts.py:104`](../../browser_use/agent/prompts.py)). Sensitive-data filtering happens on the state slot only (`_filter_sensitive_data`), since context/system slots don't carry post-substitution secrets.

**Compaction** (`maybe_compact_messages`, [`message_manager/service.py:213`](../../browser_use/agent/message_manager/service.py)) is a two-gate LLM summarizer: it fires only when `step_number - last_compaction_step >= compact_every_n_steps` **and** the rendered history exceeds `trigger_char_count` (default 40 000). It summarizes older items into `compacted_memory` (prefixed `<compacted_memory>` with an explicit "treat as unverified" warning to prevent the model from claiming un-reconfirmed steps as done), then truncates `agent_history_items` to `[first] + last keep_last_items`. The compaction LLM defaults to `page_extraction_llm` then the main `llm`.

## Prompt template selection

`SystemPrompt` ([`prompts.py:28`](../../browser_use/agent/prompts.py)) picks one of eight markdown templates in [`system_prompts/`](../../browser_use/agent/system_prompts/) from three booleans — `is_browser_use_model` (`'browser-use/'` in model name), `is_anthropic` (`isinstance(llm, ChatAnthropic)`), `flash_mode`/`use_thinking` — plus a special case for Anthropic 4.5 models (Opus/Haiku 4.5 need ≥4096-token prompts for prompt caching). Precedence: fine-tuned browser-use templates first, then Anthropic-4.5-flash, then flash-anthropic, then flash, then thinking vs. no-thinking. `override_system_message` bypasses template loading; `extend_system_message` appends. The message is created with `cache=True`.

## `multi_act` page-change guards

`multi_act(actions)` ([`service.py:2719`](../../browser_use/agent/service.py)) executes the LLM's action list sequentially but defends against acting on a **stale DOM**, since indices in `selector_map` (== `backendNodeId`) become invalid the moment the page mutates. Guards, in order:

- **`done` is single-only**: any `done` at position `i > 0` aborts before executing.
- **Inter-action delay**: `browser_profile.wait_between_actions` sleep before every action after the first.
- **Layer 1 — static flag**: after an action, if its registry entry has `terminates_sequence=True` (navigate, search, `go_back`, switch-tab — [`tools/service.py`](../../browser_use/tools/service.py)), the remaining queue is skipped.
- **Layer 2 — runtime detection**: pre-action `(url, agent_focus_target_id)` are compared to their post-action values; **any** change aborts the remainder.
- Each action runs through `tools.act(...)`; the loop breaks on `is_done`, `error`, or last index. `InterruptedError` and connection-like errors re-raise (so stop/reconnect logic upstream handles them); other exceptions are caught, appended as an `ActionResult(error=...)`, and returned **with partial results** so the model learns which actions landed.

## Failure accounting & the fallback-LLM hot-swap

`_handle_step_error()` ([`service.py:1250`](../../browser_use/agent/service.py)) classifies: `InterruptedError` → warn-and-continue; connection-like errors → wait on `browser_session._reconnect_event` if a reconnect is in flight, else mark `stopped`; everything else → increment `consecutive_failures`, format the error, and store it as `last_result`. Only **single-action** steps with an error bump `consecutive_failures` in `_post_process`; multi-action error steps are left to loop detection and replan nudges. The run aborts once failures reach `max_failures + int(final_response_after_failure)` — the `+1` gives one final `DoneAgentOutput`-forced recovery step when `final_response_after_failure` is set.

**Fallback LLM** (`_try_switch_to_fallback_llm`, [`service.py:1975`](../../browser_use/agent/service.py)): a `ModelRateLimitError`/`ModelProviderError` with a retryable status (`{401, 402, 429, 500, 502, 503, 504}`) triggers a one-time swap of `self.llm = self._fallback_llm`, registers it for token accounting, and retries `get_model_output` on the same messages. It's a latch — a second failure has no further fallback and re-raises. `_original_llm` is retained for logging.

## In-band planning

Planning is not a separate LLM pass — it rides on the same `AgentOutput`. When `enable_planning` (default true), the model may emit `plan_update: list[str]` (replace the whole plan) or `current_plan_item: int` (advance the cursor). `_update_plan_from_model_output` ([`service.py:1409`](../../browser_use/agent/service.py)) applies these to `state.plan` (a list of `PlanItem{text, status}`), marking passed items `done` and the target `current`. `_render_plan_description` renders it as a `[x]/[>]/[ ]/[-]` checklist injected back into the state message. Two nudges close the loop: `_inject_replan_nudge` (after `planning_replan_on_stall` consecutive failures) and `_inject_exploration_nudge` (after `planning_exploration_limit` steps with no plan yet).

## Loop detection

`ActionLoopDetector` ([`views.py:157`](../../browser_use/agent/views.py)) is a **soft, non-blocking** detector — it only injects awareness nudges. It keeps a rolling window (default 20) of `compute_action_hash(name, params)` values, where the hash normalizes by intent (search tokens sorted, click/input keyed by element index, navigate by full URL) so near-identical actions collide. `get_nudge_message()` escalates at repetition counts of 5/8/12. Separately, `PageFingerprint(url, element_count, text_hash)` tracks `consecutive_stagnant_pages`; ≥5 identical fingerprints emits a "page isn't changing" nudge. `wait`/`done`/`go_back` are exempt from action recording (they'd trivially self-collide). The detector state lives in `AgentState` and is thus checkpointed with the run.

## History & GIF export

Each step appends an `AgentHistory{model_output, result, state: BrowserStateHistory, metadata: StepMetadata, state_message}` to `AgentHistoryList`. `BrowserStateHistory` records URL/title/tabs, the `interacted_element` list (resolved via `AgentHistory.get_interacted_element` against the live selector map), and a **path** to the persisted screenshot (not the base64 blob). `final_result()` returns the last done action's content; `structured_output` re-parses it against `_output_model_schema` when the agent was constructed with an `output_model_schema` (the schema is also injected into the task text via `_enhance_task_with_schema`). `save_history()` serializes the list to JSON; `run()` optionally calls `create_history_gif(task, history, output_path, duration=3000, ...)` ([`gif.py:35`](../../browser_use/agent/gif.py)) to render an annotated GIF with per-step goal overlays, emitting a `CreateAgentOutputFileEvent`.

## Invariants & gotchas

- `n_steps` **starts at 1** and is incremented in `_finalize` (and defensively on step-timeout). `AgentStepInfo.step_number` is 0-indexed (`current_step = n_steps - 1`), so off-by-one care is required when comparing budgets.
- The 3-slot design means the LLM sees a *reconstructed* prompt every step, not an appended transcript — token cost scales with history-render size, which is exactly what `max_history_items` and compaction bound.
- `selector_map` indices are `backendNodeId`s and go stale on navigation; `multi_act`'s two-layer guard is the enforcement mechanism, not a nicety.
- Screenshots are always captured but only *sent* to the LLM per `use_vision`; disabling vision does not save the capture cost.
- Fallback-LLM switching is a one-shot latch per run; there is no exponential backoff or fallback chain.
- Planning/loop-detection nudges are advisory `context_messages` — they never constrain the action space. The only *hard* constraints are the `DoneAgentOutput` swap (last step / max-failure) and `max_actions_per_step` truncation.

## Rust port notes

- **The step state machine maps cleanly.** `step()`'s phase split (perceive/decide/act/finalize) is a natural `enum StepPhase` + `match`, and `AgentState` is already designed as a serializable checkpoint — a plain `#[derive(Serialize, Deserialize)]` struct. `n_steps`/`consecutive_failures`/`plan` are trivially portable.
- **Dynamic `AgentOutput` is the hard part.** Python builds a new pydantic model per page via `create_model`; Rust has no runtime type synthesis. The port must instead build the **JSON Schema** dynamically (the schema is what actually reaches the provider — see [`06-llm-provider-abstraction.md`](06-llm-provider-abstraction.md)) and deserialize actions into an untagged/adjacently-tagged `serde` enum or a `serde_json::Value` that a hand-rolled dispatcher validates. `schemars` gives static schemas; the union-of-actions needs custom schema assembly. The beta Rust bridge ([`11-beta-rust-bridge.md`](11-beta-rust-bridge.md)) already treats the action list as JSON-RPC params, which is the pragmatic model.
- **Signals & cancellation**: `SignalHandler`'s task-name-pattern cancellation is very asyncio-specific. In Rust use `tokio::signal::ctrl_c` + a `CancellationToken`; the two-stage "pause then hard-exit" is a small state machine. `asyncio.wait_for` timeouts become `tokio::time::timeout`.
- **MessageManager** is pure data transformation (string/JSON assembly, rolling windows, char-count gates) — ports directly; the compaction and page-fingerprint hashing are just `sha2`. No async needed except the compaction `ainvoke`.
- **Loop detection** is pure functions over hashes — a clean, well-isolated module (`std::collections::VecDeque` for the window).
- **Watch for**: the URL-shortening recursive walk over an arbitrary pydantic tree relies on runtime reflection; in Rust operate on `serde_json::Value` instead. The `getattr`/`setattr(action_instance, 'done', ...)` synthetic-noop trick has no direct analog — construct the enum variant explicitly.
