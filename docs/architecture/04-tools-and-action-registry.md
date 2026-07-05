# Tools, Action Registry & ActionResult

The Tools layer is the boundary between LLM intent and browser effect: a `Registry` reflects plain async Python functions into dynamic pydantic action models (a `RootModel`-union tool schema the LLM fills in), and `Tools.act()` validates, injects dependencies, substitutes secrets, and executes them under a wall-clock timeout — every action returning the uniform `ActionResult` contract that the agent folds back into its message history.

Source: [`browser_use/tools/service.py`](../../browser_use/tools/service.py) (facade + built-in actions), [`browser_use/tools/registry/service.py`](../../browser_use/tools/registry/service.py) (reflection engine), [`browser_use/tools/registry/views.py`](../../browser_use/tools/registry/views.py) (registry models), [`browser_use/tools/views.py`](../../browser_use/tools/views.py) (action param models).

## The `Tools` / `Controller` facade

`class Tools(Generic[Context])` ([`service.py:441`](../../browser_use/tools/service.py)) owns a single `Registry[Context]` and registers all built-in actions in `__init__` by decorating closures with `@self.registry.action(...)`. `Controller = Tools` ([`service.py:2313`](../../browser_use/tools/service.py)) is a backward-compat alias — same object, older name.

Construction knobs:
- `exclude_actions: list[str]` — names dropped before registration (never enter the registry).
- `output_model: type[T] | None` — swaps the free-text `done` action for a structured-output variant (see below).
- `display_files_in_done_text: bool` — whether `done` inlines file contents into the user-facing text.

The built-in action set spans navigation (`search`, `navigate`, `go_back`, `wait`), interaction (click via `_register_click_action`, `input`, `scroll`, `send_keys`, dropdowns, `upload_file`), tabs (`switch_tab`, `close_tab`), zero-LLM page tools (`search_page`, `find_elements`, `scroll_to_text`), capture (`screenshot`, `save_as_pdf`), the LLM-backed `extract`, and the terminal `done`. Custom actions attach via `@tools.action(description, ...)` ([`service.py:2088`](../../browser_use/tools/service.py)), which simply delegates to the registry decorator.

`Tools.__getattr__` ([`service.py:2254`](../../browser_use/tools/service.py)) gives ergonomic direct calls — `await tools.navigate(url=..., browser_session=...)` — by wrapping the named action in a one-off `DynamicActionModel` and routing through `act()`, so tests and imperative callers get the same validation/error-normalization path as the agent.

## Registry reflection: signatures → param models

`Registry.action()` ([`service.py:290`](../../browser_use/tools/registry/service.py)) calls `_normalize_action_function_signature()` ([`service.py:74`](../../browser_use/tools/registry/service.py)), which is the heart of the reflection engine. Two things come out: a `normalized_wrapper` that accepts *only* keyword args, and the pydantic `param_model` describing the LLM-visible parameters.

### Special-parameter dependency-injection allowlist

`_get_special_param_types()` ([`service.py:56`](../../browser_use/tools/registry/service.py)) defines the closed set of parameter names the framework will inject rather than expose to the LLM:

`context`, `browser_session` (`BrowserSession`), `page_url` (`str`), `cdp_client`, `page_extraction_llm` (`BaseChatModel`), `available_file_paths` (`list`), `has_sensitive_data` (`bool`), `file_system` (`FileSystem`), `extraction_schema`. The canonical model is `SpecialActionParameters` ([`views.py:149`](../../browser_use/tools/registry/views.py)); `get_browser_requiring_params()` marks the subset (`browser_session`, `cdp_client`, `page_url`) that can't be satisfied without a live session.

Normalization scans the function signature, and for any parameter whose name is in this allowlist it validates the annotation is type-compatible with the expected injected type (unwrapping `Optional`, allowing subclasses and `list[T]` vs `list`); a mismatch raises `ValueError` — you cannot shadow an injected name with an incompatible type. `**kwargs` in an action signature is rejected outright.

### Type-1 vs Type-2 param models

Two authoring styles are supported and disambiguated by the *first* parameter:

- **Type-1 (explicit `param_model`)**: `async def navigate(params: NavigateAction, browser_session)`. The decorator is given `param_model=NavigateAction`; the first non-special positional is the whole model. Used by nearly all built-ins because the model carries `Field(...)` descriptions and validators the LLM sees.
- **Type-2 (inferred)**: `async def wait(seconds: int = 3)`. No `param_model` given, so `create_model(f'{fn}_Params', __base__=ActionModel, **{name: (annotation, default)})` synthesizes one from the action (non-special) parameters ([`service.py:149`](../../browser_use/tools/registry/service.py)).

`normalized_wrapper` ([`service.py:169`](../../browser_use/tools/registry/service.py)) rebuilds the positional call: it iterates the *original* parameter order, appending the params model for the Type-1 slot, injected values (or defaults) for special names, and `params_dict[name]` for action fields. Required-but-missing special params raise targeted errors (`Action X requires browser_session but none provided.`). Sync functions are off-loaded via `asyncio.to_thread`; async are awaited directly. The wrapper's `__signature__` is rewritten to `(*, params=None, <special...>, **kwargs)` so downstream introspection sees the kwargs-only contract.

`RegisteredAction` ([`views.py:14`](../../browser_use/tools/registry/views.py)) is the stored record: `name`, `description`, `function` (the wrapper), `param_model`, `domains`, and `terminates_sequence`.

## `create_action_model`: the RootModel-Union tool schema

`create_action_model()` ([`service.py:507`](../../browser_use/tools/registry/service.py)) builds the schema the LLM fills. For each available action it makes a one-field model — `create_model(f'{Name}ActionModel', __base__=ActionModel, **{name: (param_model, Field(description=...))})` — so an emitted action is `{"navigate": {"url": "...", "new_tab": false}}` rather than the every-action-nullable shape. The individual models are combined:

- 0 actions → `EmptyActionModel`.
- 1 action → that model directly (no union).
- ≥2 → `class ActionModelUnion(RootModel[Union[...]])` whose `get_index`/`set_index`/`model_dump` delegate to `self.root`, then renamed to `ActionModel` for clean schemas.

`ActionModel` ([`views.py:59`](../../browser_use/tools/registry/views.py), `extra='forbid'`) provides `get_index()`/`set_index()` — the agent uses these to read/rewrite the target element index (the DOM-serializer's `backendNodeId`, per doc [`03-dom-perception-pipeline.md`](03-dom-perception-pipeline.md)) across the whole action set uniformly. `AgentOutput.type_with_custom_actions()` ([`browser_use/agent/views.py:418`](../../browser_use/agent/views.py)) splices this union into `action: list[ActionModel]`, giving the LLM its structured-output schema (doc [`05-agent-control-loop.md`](05-agent-control-loop.md)).

## Domain filtering

Actions may declare `domains` (alias `allowed_domains`; specifying both raises). `create_action_model(page_url=...)` and `get_prompt_description(page_url=...)` filter on it via `ActionRegistry._match_domains()` ([`views.py:96`](../../browser_use/tools/registry/views.py)), which globs each pattern with `match_url_with_domain_pattern`. The rule is asymmetric by design: with **no** `page_url`, only unfiltered actions are exposed (system-prompt view); with a `page_url`, `get_prompt_description` returns *only* the domain-scoped matches (they're appended per-step), while `create_action_model` returns unfiltered + matching. This keeps domain-specific tools out of the base prompt and injects them only on relevant pages.

## `execute_action`: validation, secret substitution, DI

`Registry.execute_action()` ([`service.py:328`](../../browser_use/tools/registry/service.py)) is the single execution funnel:

1. **Validate** — `action.param_model(**params)`; failure becomes a `ValueError` naming the bad params.
2. **Secret substitution** — if `sensitive_data` is present, `_replace_sensitive_data()` ([`service.py:417`](../../browser_use/tools/registry/service.py)) runs. It resolves the *current URL* from the session's focused target, selects applicable secrets (new format `{domain_pattern: {key: value}}` gated by `match_url_with_domain_pattern` against the current URL; legacy `{key: value}` exposed everywhere), then recursively rewrites `<secret>label</secret>` tags — and bare literal placeholder strings the LLM forgot to tag — in the validated model dump. **TOTP:** any placeholder whose label ends with `bu_2fa_code` is treated as a TOTP seed and replaced with `pyotp.TOTP(seed, digits=6).now()`. Missing placeholders are logged, not fatal.
3. **Build `special_context`** — assembles the injectable dict: `browser_session`, `page_extraction_llm`, `available_file_paths`, `has_sensitive_data` (only true for `input` with secrets), `file_system`, `extraction_schema`, plus `page_url` (`await get_current_page_url()`) and `cdp_client` when a session exists. Raw `sensitive_data` is passed *only* to the `input` action.
4. **Invoke** — `await action.function(params=validated_params, **special_context)`; the wrapper ignores injected names the action didn't ask for. Errors are wrapped in `RuntimeError`, except the browser/LLM "requires X but none provided" messages which are surfaced verbatim.

## The uniform `ActionResult` contract

Every action returns `ActionResult` ([`browser_use/agent/views.py:307`](../../browser_use/agent/views.py)) (or a bare `str`/`None` that `act()` coerces). Fields split three ways by lifetime:

- **Control**: `is_done`, `success` (a `model_validator` forbids `success=True` unless `is_done=True`), `error`.
- **Memory routing**: `long_term_memory` (persisted every step), `extracted_content` + `include_extracted_content_only_once` (surfaced to the next step's `<read_state>` then dropped — how large `extract`/`find_elements` payloads avoid bloating history), the deprecated `include_in_memory`.
- **Payload/observability**: `attachments`, `images` (base64, kept out of the text channel), `metadata`, `judgement`.

This is the invariant the whole agent loop leans on: actions are effect-ful but return a single normalized value, so `multi_act` can treat them homogeneously.

## The `extract` action: markdown → chunk → `page_extraction_llm`

`extract` ([`service.py:1055`](../../browser_use/tools/service.py)) is the one built-in that spends an LLM call. Flow:

1. `extract_clean_markdown()` ([`browser_use/dom/markdown_extractor.py`](../../browser_use/dom/markdown_extractor.py)) renders the page to filtered markdown, returning `content_stats` (HTML → initial → filtered char counts). `extract_images` auto-enables on image-keyword queries.
2. `chunk_markdown_by_structure(content, max_chunk_chars=100000, start_from_char=...)` splits structure-aware (not naive truncation); the first chunk is used, `has_more`/`char_offset_end` drive the `start_from_char` continuation contract and an `overlap_prefix` re-prepends context like table headers. `start_from_char` past the end returns an error result.
3. Surrogates are sanitized, then one of two prompt paths runs, each guarded by `asyncio.wait_for(..., timeout=120.0)`:
   - **Structured** — when the LLM passed `output_schema` or the agent injected `extraction_schema`, `schema_dict_to_pydantic_model()` ([`browser_use/tools/extraction/schema_utils.py`](../../browser_use/tools/extraction/schema_utils.py)) compiles it to a pydantic model (rejecting `$ref`/`allOf`/`anyOf`/etc. composition, falling back to free-text on failure) passed as `output_format=`. Result is JSON-serialized with an `ExtractionResult` metadata blob.
   - **Free-text** — plain `ainvoke` returning prose.
4. Result memory: payloads under 10k chars go inline (`include_extracted_content_only_once=False`); larger ones are written to the filesystem VFS via `file_system.save_extracted_content()` and referenced once. Output is wrapped in `<url>/<query>/<result>` tags.

`page_extraction_llm` is a *separate, injected* model (often cheaper) from the agent's reasoning LLM — the DI allowlist is what makes that substitution invisible to the action body.

## `act()`: the timeout guard and result normalization

`Tools.act()` ([`service.py:2164`](../../browser_use/tools/service.py)) is the per-action executor the agent's `multi_act` calls. It resolves `timeout_s` via `_coerce_valid_action_timeout()` (default `_DEFAULT_ACTION_TIMEOUT_S`, from `BROWSER_USE_ACTION_TIMEOUT_S` or `180.0`; `nan`/`inf`/`<=0` are rejected with a warning — those would defeat the guard by timing out instantly or never). The single-action model is dumped, and for its one entry:

- Wrapped in a Laminar `TOOL` span when `lmnr` is installed, else a `nullcontext`.
- `result = await asyncio.wait_for(registry.execute_action(...), timeout=timeout_s)`.
- `BrowserError` → `handle_browser_error()` ([`service.py:185`](../../browser_use/tools/service.py)), which maps `short_term_memory`/`long_term_memory` onto `ActionResult` fields (re-raising only if neither is set). `TimeoutError` (the outer cap *or* the inner 120s extract cap bubbling up) → a recovery `ActionResult(error=...)` telling the agent the CDP socket may be dead. Any other exception → `ActionResult(error=str(e))`.
- Return normalization: `str` → `ActionResult(extracted_content=...)`, `ActionResult` passthrough, `None` → empty result, anything else raises.

The 180s default deliberately sits above the 120s inner `page_extraction_llm` cap so slow-but-valid LLM actions aren't truncated. **Invariant:** `act()` never raises for action-level failures — it always yields an `ActionResult`, so the agent loop can always continue or recover.

### `terminates_sequence` and the DOM-staleness guard

`act()` executes exactly one action; batching lives in `Agent.multi_act()` ([`browser_use/agent/service.py:2718`](../../browser_use/agent/service.py)). Two guards abort the remaining queue when the page shifts under it: **(1)** a static flag — actions registered with `terminates_sequence=True` (`navigate`, `search`, `go_back`, tab switches) — and **(2)** runtime detection comparing pre/post URL and focused-target id. Both exist because the element indices in queued actions are `backendNodeId`s valid only against the DOM snapshot they were planned on; a navigation invalidates them. `done` is additionally forced to be a solo action.

## Gotchas

- **Index == backendNodeId.** `get_index`/`set_index` assume the action param is named `index`; the value is the DOM serializer's `backendNodeId`, not a list position.
- **Silent injection.** Adding a parameter named in the special allowlist makes it framework-injected and *invisible to the LLM* — a footgun if you meant it as a real argument.
- **Legacy secrets are global.** Old-format `{key: value}` sensitive data is exposed on every domain; only the `{domain: {...}}` form is URL-scoped.
- **Success coupling.** `success=True` without `is_done=True` fails validation — only `done` can report task success.
- **Two inference styles coexist.** Type-1 needs `param_model=`; Type-2 infers from the signature but loses the `Field` descriptions the LLM benefits from.

## Rust port notes

- **Maps cleanly.** `ActionResult` and the param models are plain data — `serde` structs with `#[serde(deny_unknown_fields)]` mirroring `extra='forbid'`. The domain-glob filter, TOTP substitution (`totp-rs`), secret-tag regex, and the timeout guard (`tokio::time::timeout` around each action) are mechanical translations. The `RootModel`-union tool schema corresponds to a `#[serde(untagged)]` or externally-tagged enum, one variant per action, feeding the provider's structured-output schema.
- **Needs redesign.** Python's runtime *signature reflection* (`inspect.signature`, dynamic `create_model`, `__signature__` rewriting) has no direct Rust analog — Rust lacks runtime introspection. The idiomatic port registers actions via a `trait Action { fn schema() -> Schema; async fn run(&self, params, ctx: &SpecialContext) -> ActionResult; }` with a proc-macro (`#[action(description = "...")]`) deriving the param struct's JSON schema at compile time, and the special-param DI allowlist becoming an explicit `SpecialContext` struct passed to every handler rather than name-matched kwargs. This trades dynamism for type safety, which is a net win here. The beta Rust bridge ([`11-beta-rust-bridge.md`](11-beta-rust-bridge.md)) already carries actions as JSON-RPC method payloads — its wire format is the authoritative contract for a native tool layer.

See also: [`index.md`](index.md), [`03-dom-perception-pipeline.md`](03-dom-perception-pipeline.md) (index/`backendNodeId`), [`05-agent-control-loop.md`](05-agent-control-loop.md) (`multi_act`, `AgentOutput`), [`06-llm-provider-abstraction.md`](06-llm-provider-abstraction.md) (`page_extraction_llm`, structured output).
