# Cross-Cutting Services

The eight satellite subsystems that hang off the Agent/BrowserSession core but aren't part of the perceive→decide→act loop: token accounting, cloud sync + device-grant OAuth, the in-memory file VFS, the remote code sandbox, PostHog telemetry, remote skills, Gmail 2FA, and screenshot persistence. Each is a self-contained module under `browser_use/{tokens,sync,filesystem,sandbox,telemetry,skills,integrations,screenshots}/`; none touch CDP, and most are optional and network-facing.

See [index.md](index.md) for the layer map. These services are wired in by the Agent orchestrator ([05-agent-control-loop.md](05-agent-control-loop.md)) and consume LLM usage records ([06-llm-provider-abstraction.md](06-llm-provider-abstraction.md)) and bus events ([02-event-bus-and-watchdogs.md](02-event-bus-and-watchdogs.md)).

---

## TokenCost: ainvoke monkeypatch + pricing cache

[`browser_use/tokens/service.py`](../../browser_use/tokens/service.py) tracks per-invocation token usage and (optionally) dollar cost. The mechanism is a **runtime monkeypatch of each LLM's `ainvoke`**.

### Registration & the wrapper

`TokenCost.register_llm(llm: BaseChatModel) -> BaseChatModel` ([service.py:352](../../browser_use/tokens/service.py)) closes over the original bound method and swaps in a tracking wrapper:

```python
original_ainvoke = llm.ainvoke
async def tracked_ainvoke(messages, output_format=None, **kwargs):
    result = await original_ainvoke(messages, output_format, **kwargs)
    if result.usage:
        usage = token_cost_service.add_usage(llm.model, result.usage)
        create_task_with_error_handling(token_cost_service._log_usage(...), suppress_exceptions=True)
    return result
object.__setattr__(llm, 'ainvoke', tracked_ainvoke)   # bypass Pydantic's frozen setattr
```

- **De-dup by `id(llm)`**: the registry key is `str(id(llm))`, so the same instance registered twice (agent registers `llm`, `page_extraction_llm`, `judge_llm`, the compaction LLM, and any fallback LLM — see [agent/service.py:422-427](../../browser_use/agent/service.py)) is patched only once.
- `object.__setattr__` is required because `BaseChatModel` subclasses are Pydantic models that reject normal attribute assignment.
- Logging is fired-and-forgotten via `create_task_with_error_handling(...)` so accounting never blocks the step loop.

### Pricing sources (checked in order)

`get_model_pricing(model_name)` resolves a `ModelPricing` ([tokens/views.py](../../browser_use/tokens/views.py)) from a 4-tier cascade:

1. **`CUSTOM_MODEL_PRICING`** ([custom_pricing.py](../../browser_use/tokens/custom_pricing.py)) — hardcoded per-token prices for models LiteLLM doesn't list (e.g. `bu-1-0`).
2. **OpenRouter** — if the id is `openrouter/…` / `openrouter-…` (`is_openrouter_pricing_model`), fetch from `https://openrouter.ai/api/v1/models` via [openrouter_pricing.py](../../browser_use/tokens/openrouter_pricing.py). OpenRouter publishes prices as **per-token strings**; a module-level dict cache (`_OPENROUTER_MODELS_CACHE`) holds them for 1 hour (`OPENROUTER_MODELS_CACHE_SECONDS`).
3. **LiteLLM pricing file** — the bulk source, `model_prices_and_context_window.json` fetched from the BerriAI/litellm GitHub raw URL. `MODEL_TO_LITELLM` ([mappings.py](../../browser_use/tokens/mappings.py)) rewrites a handful of names (e.g. `gemini-flash-latest` → `gemini/gemini-flash-latest`) before lookup.
4. Fallback OpenRouter lookup for anything still unresolved.

### The on-disk cache

- Location: `xdg_cache_home() / 'browser_use/token_cost'` (respects `XDG_CACHE_HOME`).
- TTL: `CACHE_DURATION = timedelta(days=1)`. Files are `pricing_<YYYYMMDD_HHMMSS>.json` holding a `CachedPricingData{timestamp, source_url, data}`.
- **Source-URL scoping**: `_cache_source_matches` keeps caches from different `pricing_url`s from evicting each other; a `source_url=None` legacy cache is only reused when the current URL is the default. `_find_valid_cache` sorts by mtime, returns the newest still-valid file, and deletes expired ones. `clean_old_caches(keep_count=3)` prunes same-source files.
- Lazy init: `include_cost` gates everything (env `BROWSER_USE_CALCULATE_COST` or the `calculate_cost` ctor arg). If false, `initialize()` never fetches and cost is always `None` — usage tokens are still recorded.

### Cost math gotchas

`calculate_cost(model, usage: ChatInvokeUsage)` ([service.py:221](../../browser_use/tokens/service.py)) separates **uncached prompt tokens**, **cache-read** (`cache_read_input_token_cost`), and **cache-creation** tokens, with a distinct 5-minute vs 1-hour Anthropic cache-creation rate (`cache_creation_1h_input_token_cost` falling back to the 5m rate). A `usage.pricing_multiplier` (default 1.0) scales every term. `UsageSummary` recomputes costs record-by-record on demand rather than storing dollar figures, so a late pricing refresh retroactively re-prices history.

---

## CloudSync + OAuth2 device-grant auth

[`browser_use/sync/`](../../browser_use/sync/) implements event forwarding to the Browser Use cloud plus the OAuth2 **Device Authorization Grant** (RFC 8628) used to obtain an API token from a headless CLI.

> **Wiring status**: the `CloudSync` service and `DeviceAuthClient` are still present and importable (`from browser_use.sync import CloudSync`), but the Agent no longer instantiates them — `Agent.authenticate_cloud_sync()` ([agent/service.py:4025](../../browser_use/agent/service.py)) is now a stub that logs "Cloud sync has been removed" and returns `False`. The cloud **event models** in [`agent/cloud_events.py`](../../browser_use/agent/cloud_events.py) (`CreateAgentSessionEvent`, `UpdateAgentTaskEvent`, …) still exist and defensively guard `agent.cloud_sync` with `hasattr`. Document the module as the reusable primitive it is; treat the agent-level integration as dormant.

### CloudSync ([sync/service.py](../../browser_use/sync/service.py))

`handle_event(event: BaseEvent)` is a bus handler that POSTs events to `{base_url}/api/v1/events` (batch of one). Behavior branches on auth:

- Authenticated → send every event, stamping `user_id` from the auth client (unless it's already the sentinel `TEMP_USER_ID`).
- Not authenticated but `allow_session_events_for_auth=True` → forward events anyway (this is how a pre-auth session bootstraps a shareable cloud URL); a `CreateAgentSessionEvent` flips `auth_flow_active`.
- Otherwise drop silently.

Every path swallows exceptions and logs at debug — cloud sync must never break a run. `device_id` is attached to each serialized event.

### DeviceAuthClient ([sync/auth.py](../../browser_use/sync/auth.py))

- **client_id** `'library'`, **scope** `'read write'`.
- **Device id**: `get_or_create_device_id()` persists a `uuid7str()` at `BROWSER_USE_CONFIG_DIR/device_id` (the same file PostHog reads for its distinct id — see below).
- **Token storage**: `CloudAuthConfig{api_token, user_id, authorized_at}` serialized to `BROWSER_USE_CONFIG_DIR/cloud_auth.json`, `chmod 0o600`.
- **Flow**: `start_device_authorization()` POSTs to `/api/v1/oauth/device/authorize` → `{device_code, user_code, verification_uri, verification_uri_complete, interval}`. `poll_for_token()` polls `/api/v1/oauth/device/token` with `grant_type=urn:ietf:params:oauth:grant-type:device_code`, honoring the standard `authorization_pending` (keep waiting) and `slow_down` (back off `interval`) error codes, up to a 1800 s timeout. On success the token is saved.
- `authenticate()` rewrites the backend `verification_uri` to a frontend URL (`BROWSER_USE_CLOUD_UI_URL`, or `//api.` → `//cloud.`) before printing the "view this run" link.

Config knobs: `BROWSER_USE_CLOUD_SYNC` (defaults to `ANONYMIZED_TELEMETRY`), `BROWSER_USE_CLOUD_API_URL`, `BROWSER_USE_CLOUD_UI_URL` ([config.py:62-76](../../browser_use/config.py)).

---

## FileSystem: in-memory VFS

[`browser_use/filesystem/file_system.py`](../../browser_use/filesystem/file_system.py) is a typed, in-memory virtual file system the agent uses for scratch/output files (`write_file`, `read_file`, `append_file`, `replace_file_str`, `save_extracted_content`). It shadows a real directory on disk for durability but treats memory as the source of truth.

### Layout & types

- Ctor scrubs and recreates `base_dir / 'browseruse_agent_data'` (`DEFAULT_FILE_SYSTEM_PATH`) on every construction — a fresh sandbox per agent. A `todo.md` default file is seeded.
- File classes derive from `BaseFile(BaseModel, ABC)` keyed by extension: `md, txt, json, jsonl, csv, pdf, docx, html, xml`. Content lives as a `str` in memory; `sync_to_disk`/`sync_to_disk_sync` mirror to disk (async writes go through a `ThreadPoolExecutor`).
- **PDF/DOCX are write-only renderers**: `PdfFile.sync_to_disk_sync` lazily imports `reportlab` and renders markdown-ish headings to a PDF; `DocxFile` uses `python-docx`. In-memory `content` stays plain text.
- **CSV self-heals**: `CsvFile._normalize_csv` round-trips every write through Python's `csv` module to fix LLM-produced malformed CSV, including un-escaping double-escaped JSON (`\\n`, `\\"`) when the payload has no real newlines.

### Safety invariants

- `_is_valid_filename` enforces a regex allowing alphanumerics, `_ - . ( )`, spaces, and CJK (`一-鿿`) before a supported extension.
- **Directory traversal is neutralized** by `os.path.basename()` in `_resolve_filename` (so `../secret.md` collapses to `secret.md`), which also attempts `sanitize_filename` and reports the auto-correction back to the LLM.
- `UNSUPPORTED_BINARY_EXTENSIONS` (png/jpg/zip/exe/…) triggers a targeted error telling the LLM screenshots are auto-captured, not file-written.
- `read_file_structured(external_file=True)` is the escape hatch to read arbitrary on-disk files: text is inlined; DOCX via `python-docx`; images (`jpg/jpeg/png`) returned as base64; **large PDFs are ranked by TF-IDF** (`math.log(num_pages/pages_with_word)`) to fit the highest-signal pages under a 60 000-char budget.

### Persistence

`get_state() -> FileSystemState` and `FileSystem.from_state(state)` (de)serialize `{full_filename: {type, data}}` so the whole VFS survives agent pause/resume; `from_state` reconstructs each `BaseFile` subclass by class-name lookup and re-syncs to disk at the identical `base_dir`. The Agent restores from state if present, else builds fresh ([agent/service.py:700-728](../../browser_use/agent/service.py)). `nuke()` deletes the data dir.

---

## Remote sandbox over SSE

[`browser_use/sandbox/sandbox.py`](../../browser_use/sandbox/sandbox.py) exposes an `@sandbox(...)` decorator that ships a local `async def task(browser: Browser, ...)` to `https://sandbox.api.browser-use.com/sandbox-stream` and streams execution back over **Server-Sent Events**. It is a remote-code-execution facade: the browser runs in the cloud, the function body runs there too, only params and results cross the wire.

### How a function is teleported

The decorator validates the first param is `browser: Browser`, then at call time ([sandbox.py:285](../../browser_use/sandbox/sandbox.py)):

1. **Param capture** — `_extract_all_params` gathers explicit args, `__closure__` freevars (expanding `self.__dict__`), and referenced `__globals__`. Everything is `cloudpickle.dumps`-ed and base64-encoded.
2. **Source extraction** — `_get_function_source_without_decorator` re-parses the AST and strips decorators; `_get_imports_used_in_function` walks `co_names` + type annotations (recursing through `Union`, `Literal`, and Pydantic generics via `__pydantic_generic_metadata__`) to emit only the imports the body actually needs.
3. **Codegen** — a wrapper module is synthesized that unpickles `_params`, injects closure/global vars at module scope, redefines the function, and exposes `async def run(browser)`. Both the code and pickled params are base64'd into the JSON payload alongside `cloud_profile_id`, `cloud_proxy_country_code`, `cloud_timeout`, and `env`.

### SSE protocol ([sandbox/views.py](../../browser_use/sandbox/views.py))

`httpx.AsyncClient(timeout=1800).stream('POST', ...)` iterates `data: ` lines, each parsed by `SSEEvent.from_json` into a discriminated union over `SSEEventType`:

| Event | Payload | Handling |
| --- | --- | --- |
| `browser_created` | `BrowserCreatedData{session_id, live_url}` | prints the clickable live-view URL; fires `on_browser_created` |
| `instance_ready` | — | "browser ready" |
| `log` | `LogData{message, level}` | routes stdout/stderr/info to console; scrapes `$credits` |
| `result` | `ResultData{execution_response}` | success → capture result; failure → raise `SandboxError` |
| `error` | `ErrorData{error, traceback}` | raise `SandboxError` |

Auth is `X-API-Key: BROWSER_USE_API_KEY`. On completion, `_parse_with_type_annotation` reconstructs the declared return type **without validation** — recursively rebuilding Pydantic v2 (`model_construct`) / v1 (`construct`) models, dataclasses, enums, and generics, with special handling to re-attach `AgentHistoryList._output_model_schema` from the generic arg. The wrapper's `__signature__`/`__annotations__` are rewritten to drop `browser` so callers never pass it.

---

## PostHog telemetry singleton

[`browser_use/telemetry/service.py`](../../browser_use/telemetry/service.py) captures anonymized product events.

- `ProductTelemetry` is a **process-wide singleton** via the `@singleton` decorator ([utils.py:472](../../browser_use/utils.py)) — a closure holding one instance, no thread-safety (fine given the GIL + construction-at-import).
- Disabled when `ANONYMIZED_TELEMETRY` is false ([config.py:58](../../browser_use/config.py)); then `_posthog_client is None` and `capture`/`flush` are no-ops.
- **Distinct id** is read from `BROWSER_USE_CONFIG_DIR/device_id` (shared with `DeviceAuthClient`); missing → a new `uuid7str()` is written; any file error degrades to `'UNKNOWN_USER_ID'`.
- Hardcoded `PROJECT_API_KEY` + EU host `https://eu.i.posthog.com`, `enable_exception_autocapture=True`. PostHog's own logger is silenced unless `BROWSER_USE_LOGGING_LEVEL=debug`.
- Event schema is dataclass-based ([telemetry/views.py](../../browser_use/telemetry/views.py)): `AgentTelemetryEvent`, `MCPClientTelemetryEvent`, `MCPServerTelemetryEvent`, `CLITelemetryEvent`. `BaseTelemetryEvent.properties` auto-injects `is_docker`. `capture()` never blocks — PostHog batches internally — and swallows all send errors.

---

## Remote skills

[`browser_use/skills/service.py`](../../browser_use/skills/service.py) `SkillService` fetches and executes pre-built "skills" (parameterized cloud automations) through the `browser_use_sdk.AsyncBrowserUse` client. This is distinct from the local *skill install* CLI in `skills/install.py`.

- Ctor takes `skill_ids: list[str | '*']` and an API key (`BROWSER_USE_API_KEY`). `async_init()` pages `list_skills(page_size=100, is_enabled=True)`; the `'*'` wildcard is **capped at the first 100** to avoid flooding the LLM's action space, while explicit ids paginate up to 5 pages until all are found. Only `status == 'finished'` skills are kept.
- SDK `SkillResponse`s are wrapped in the local `Skill` model ([skills/views.py](../../browser_use/skills/views.py)), which exposes `parameters_pydantic(exclude_cookies=...)` and `output_type_pydantic` to synthesize Pydantic models from JSON parameter/output schemas — that's how a skill becomes a dynamic agent action ([agent/service.py:2561 `_register_skills_as_actions`](../../browser_use/agent/service.py)).
- `execute_skill(skill_id, parameters, cookies)` validates params against the skill's Pydantic schema, then **injects browser cookies into `type == 'cookie'` params**, raising `MissingCookieException` (with an obtain-it description) when a required cookie is absent. Returns the SDK `ExecuteSkillResponse`; API failures are caught and returned as a `success=False` response rather than raised.

---

## Gmail 2FA integration

[`browser_use/integrations/gmail/`](../../browser_use/integrations/gmail/) lets the agent read OTP/2FA codes and magic links from Gmail.

- `GmailService` ([service.py](../../browser_use/integrations/gmail/service.py)) authenticates read-only (`scope gmail.readonly`) via one of two paths: a **direct `access_token`** (wraps it in `google.oauth2.credentials.Credentials`) or the **file-based `InstalledAppFlow`** — `run_local_server(port=8080, open_browser=True)` opens a browser consent flow, then caches the token JSON at `BROWSER_USE_CONFIG_DIR/gmail_token.json` (credentials at `gmail_credentials.json`). Expired tokens auto-refresh via the refresh token.
- `get_recent_emails(max_results, query, time_filter='1h')` prepends a `newer_than:` Gmail search operator and returns parsed `{subject, from, body, timestamp, …}` dicts; `_extract_body` base64url-decodes `text/plain` (falling back to `text/html`) across multipart payloads.
- `register_gmail_actions(tools, gmail_service=None, access_token=None)` ([actions.py](../../browser_use/integrations/gmail/actions.py)) registers a single `get_recent_emails` action (keyword + `max_results`, hardwired to the **last 5 minutes**) onto a `Tools` registry, holding the service in a module-level global. Auth is lazy — the first action call triggers `authenticate()`. The action returns an `ActionResult` with `include_extracted_content_only_once=True` so the code isn't re-fed to the LLM every step.

---

## Screenshot persistence

[`browser_use/screenshots/service.py`](../../browser_use/screenshots/service.py) `ScreenshotService` is a thin base64↔disk store, decoupling the (potentially large) screenshot blobs from the serialized agent history.

- Writes to `agent_directory/screenshots/step_<n>.png`; `store_screenshot(b64, step_number)` decodes and writes via `anyio` async file I/O, returning the path. History stores the **path**, not the bytes.
- `get_screenshot(path) -> str | None` reloads and re-encodes to base64 on demand (e.g. for the LLM message or GIF assembly), returning `None` for missing files.
- Both methods carry `@observe_debug(ignore_input, ignore_output)` so the large payloads never hit tracing. Wired in via `Agent._set_screenshot_service` ([agent/service.py:730](../../browser_use/agent/service.py)); the DOM/screenshot watchdog produces the captures ([02-event-bus-and-watchdogs.md](02-event-bus-and-watchdogs.md)).

---

## Rust port notes

- **TokenCost monkeypatch → decorator/middleware.** Rust has no `object.__setattr__` runtime patching; model the LLM client as a trait object wrapped in a `TokenTracking<C: ChatModel>` decorator, or emit a `Usage` struct from every `ainvoke` and fold it in the caller. The pricing cache is trivial: `reqwest` + `serde_json`, an on-disk TTL cache, and a `parking_lot::RwLock<HashMap>` for the OpenRouter in-process cache. Cost math ports 1:1.
- **CloudSync / device grant.** Maps cleanly onto `reqwest` + `serde`. The RFC 8628 polling loop (`authorization_pending`/`slow_down`) is standard; use `tokio::time::sleep`. Token file with `0o600` needs `std::os::unix::fs::PermissionsExt` (and a Windows fallback). Given it's dormant in the agent, this is low-priority.
- **FileSystem VFS** ports cleanly — an `enum FileKind` with a `trait File` and a `HashMap<String, Box<dyn File>>`. PDF/DOCX rendering is the hard part: no drop-in `reportlab`/`python-docx`; consider `printpdf`/`docx-rs` or shelling out. The TF-IDF PDF reader wants a `pdf-extract`-class crate. Filename validation is a `regex` port.
- **Sandbox is the redesign risk.** The whole scheme depends on Python-specific runtime reflection: `cloudpickle` of closures/globals, `inspect.getsource` + `ast` rewriting, and validation-free Pydantic reconstruction. **None of this has a Rust analogue** — Rust can't serialize closures or re-derive source at runtime. A Rust port would need an entirely different contract: send a declarative task spec (or WASM/compiled artifact) instead of teleporting a live function. The SSE client itself (`reqwest` + `eventsource-stream`) and the typed `SSEEvent` union (`serde` tagged enum) are easy.
- **Telemetry**: a `OnceCell<Option<PostHogClient>>` for the singleton; PostHog has no official Rust SDK, so POST the capture API directly. Fire-and-forget on a `tokio` task.
- **Remote skills / Gmail** depend on external SDKs (`browser_use_sdk`, Google API client) with no Rust equivalents — reimplement over raw REST (`reqwest`) against the same endpoints. Gmail's `run_local_server` OAuth loopback needs a tiny `hyper`/`axum` listener on `:8080`.
- **Screenshots** are trivial: `tokio::fs` + the `base64` crate.
- **Shared gotcha**: several services co-own `BROWSER_USE_CONFIG_DIR/device_id`. Centralize that in the config layer ([09-configuration-logging-bootstrap.md](09-configuration-logging-bootstrap.md)) so telemetry and auth read one canonical id.
