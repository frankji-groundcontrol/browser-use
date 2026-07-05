# Configuration, Logging & Process Bootstrap

The satellite layer that runs before any browser or LLM: a three-source config resolver ([`config.py`](../../browser_use/config.py)), Chrome launch-arg + display-geometry synthesis ([`browser/profile.py`](../../browser_use/browser/profile.py)), a custom logging setup with a `RESULT` level ([`logging_config.py`](../../browser_use/logging_config.py)), and the import-time side effects in the package `__init__` plus the `cli.py` dispatch surface. None of it is on the hot path, but all of it decides *what* the hot path sees.

See the [architecture index](index.md); the CLI's default "run" path hands off to the native binary described in [`11-beta-rust-bridge.md`](11-beta-rust-bridge.md).

## Three-layer configuration

`browser-use` carries three overlapping config representations that accreted over versions. They are unified behind one singleton, `CONFIG`, defined at the bottom of [`config.py`](../../browser_use/config.py).

### Layer 1 — `OldConfig`: live-env properties

[`OldConfig`](../../browser_use/config.py) is a plain class whose every setting is a `@property` that calls `os.getenv(...)` at access time. There is no caching of values — `CONFIG.BROWSER_USE_LOGGING_LEVEL` re-reads the environment on every read, so mutating `os.environ` mid-process is observable. Booleans use the idiom `os.getenv(name, default).lower()[:1] in 'ty1'` (accepts `t`/`true`/`y`/`yes`/`1`). Notable derived logic:

- `BROWSER_USE_CLOUD_SYNC` defaults to the value of `ANONYMIZED_TELEMETRY`.
- Path props (`XDG_CONFIG_HOME`, `BROWSER_USE_CONFIG_DIR`, `BROWSER_USE_PROFILES_DIR`, `BROWSER_USE_EXTENSIONS_DIR`) `.expanduser().resolve()` and trigger `_ensure_dirs()`, which `mkdir(parents=True, exist_ok=True)` the config/profiles/extensions tree exactly once (guarded by the class-level `_dirs_created` flag).
- `IN_DOCKER` ORs the `IN_DOCKER` env with `is_running_in_docker()` — a `@cache`d heuristic that checks `/.dockerenv`, `docker` in `/proc/1/cgroup`, whether PID 1's cmdline looks like `py`/`uv`/`app`, and whether the machine has `< 10` total PIDs.

### Layer 2 — `FlatEnvConfig`: pydantic-settings snapshot

[`FlatEnvConfig(BaseSettings)`](../../browser_use/config.py) is a `pydantic_settings` model with `SettingsConfigDict(env_file='.env', case_sensitive=True, extra='allow')`. Instantiating it loads `.env` + typed env vars into a validated object. It is the *only* place several newer vars live — the MCP-specific (`BROWSER_USE_CONFIG_PATH`, `BROWSER_USE_HEADLESS`, `BROWSER_USE_ALLOWED_DOMAINS`, `BROWSER_USE_LLM_MODEL`), proxy (`BROWSER_USE_PROXY_URL`/`_NO_PROXY`/`_USERNAME`/`_PASSWORD`), and `BROWSER_USE_DISABLE_EXTENSIONS`. Each read constructs a fresh `FlatEnvConfig()`, so it too re-reads env/`.env` per access rather than being a true singleton snapshot.

### Layer 3 — `config.json`: uuid-keyed DB-style entries + migration

The on-disk config is a "database-style" JSON with three tables keyed by uuid4, modeled by [`DBStyleConfigJSON`](../../browser_use/config.py): `browser_profile: dict[str, BrowserProfileEntry]`, `llm: dict[str, LLMEntry]`, `agent: dict[str, AgentEntry]`. Each entry derives from `DBStyleEntry` (`id`, `default: bool`, `created_at` iso-timestamp); `BrowserProfileEntry` sets `extra='allow'` so it absorbs arbitrary `BrowserProfile` fields.

[`load_and_migrate_config(config_path)`](../../browser_use/config.py) is the migration gate:

1. Missing file → `create_default_config()` (a default profile with `headless=False`, an `LLMEntry(model='gpt-4.1-mini', api_key='your-openai-api-key-here')`, a default agent), written with `indent=2`.
2. Existing file is treated as "new format" only if all three table keys exist, are dicts, **and** the first `browser_profile` value is a dict containing an `id` key.
3. Anything else is deemed "old format" → **silently overwritten** with a fresh default config (the old data is discarded, not transformed). Any load exception falls through to the same fresh-config path.

This is destructive migration by design: there is no field-by-field porting, just detect-and-reset.

### The `CONFIG` proxy and precedence

[`Config`](../../browser_use/config.py) ties the layers together via `__getattr__`. On every attribute access it constructs a fresh `OldConfig()` and, **if that attr exists there, returns it** — so any name defined on `OldConfig` always resolves through the live-env path. Only names absent from `OldConfig` fall through to a fresh `FlatEnvConfig()`. Attributes starting with `_` raise `AttributeError` immediately. A handful of names are dispatched to bound helpers: `get_default_profile`, `get_default_llm`, `get_default_agent`, `load_config`.

`CONFIG = Config()` is the module singleton imported everywhere (`from browser_use.config import CONFIG`).

**Gotcha:** because `OldConfig` wins by name, and it has no typed coercion, a var declared in *both* layers (e.g. `BROWSER_USE_LOGGING_LEVEL`) is served by `OldConfig`, while a var only in `FlatEnvConfig` (e.g. `BROWSER_USE_HEADLESS`) gets pydantic typing. Adding a var to both places changes which coercion applies.

### MCP config assembly

`CONFIG.load_config()` → `_load_config()` builds the dict MCP components consume: it merges the default `browser_profile`/`llm`/`agent` entries (via `_get_default_*`, which pick the `default=True` entry or the first one) and then layers env overrides — `BROWSER_USE_HEADLESS`, comma-split `BROWSER_USE_ALLOWED_DOMAINS`, a consolidated `proxy` dict (`server`/`bypass`/`username`/`password`), `OPENAI_API_KEY` → `llm.api_key`, `BROWSER_USE_LLM_MODEL` → `llm.model`, and `BROWSER_USE_DISABLE_EXTENSIONS` → `enable_default_extensions` (inverted). Exposed as module fn `load_browser_use_config()`.

## BrowserProfile: launch-arg synthesis & display detection

[`BrowserProfile`](../../browser_use/browser/profile.py) is the pydantic template of everything needed to launch/connect a browser. It multiply-inherits four arg models (`BrowserConnectArgs`, `BrowserLaunchPersistentContextArgs`, `BrowserLaunchArgs`, `BrowserNewContextArgs`) that mirror the old Playwright API surface, then adds browser-use-specific fields (`disable_security`, `deterministic_rendering`, `allowed_domains`/`prohibited_domains`, `proxy: ProxySettings`, `enable_default_extensions`, highlight/timing knobs, etc.).

### Chrome argument tables

Module-level constants hold the flag sets, composed conditionally in `get_args()`:

- `CHROME_DEFAULT_ARGS` — the always-on baseline (Playwright-derived + browser-use additions), ending in `--disable-features=<CHROME_DISABLED_COMPONENTS joined>` (disables `AutomationControlled`, `BackForwardCache`, `OptimizationHints`, `Translate`, …).
- `CHROME_DOCKER_ARGS` (`--no-sandbox`, `--disable-dev-shm-usage`, `--no-zygote`, …) — applied when `CONFIG.IN_DOCKER or not chromium_sandbox`.
- `CHROME_HEADLESS_ARGS` (`--headless=new`), `CHROME_DISABLE_SECURITY_ARGS`, `CHROME_DETERMINISTIC_RENDERING_ARGS` — each gated on the matching field.

[`get_args()`](../../browser_use/browser/profile.py) is the assembler. It starts from `CHROME_DEFAULT_ARGS` minus `ignore_default_args` (or `[]` if `ignore_default_args is True`), appends user `args`, `--user-data-dir`, `--profile-directory`, the conditional tables, window-size/position (or `--start-maximized` when headful with no explicit size), proxy and user-agent flags, and extension flags. It then does two dedup passes: (1) **merge** all `--disable-features=` occurrences into a single de-duplicated flag (so `disable_security=True` can't clobber the extension-critical feature list), and (2) round-trip every remaining flag through `args_as_dict()`/`args_as_list()` to collapse duplicate keys (last value wins).

### Display detection

[`get_display_size()`](../../browser_use/browser/profile.py) (`@cache`) tries macOS `AppKit.NSScreen.mainScreen().frame()`, then `screeninfo.get_monitors()[0]` for Linux/Windows, returning a `ViewportSize | None`. [`get_window_adjustments()`](../../browser_use/browser/profile.py) returns per-platform title-bar offsets.

[`detect_display_configuration()`](../../browser_use/browser/profile.py) runs in `model_post_init` (alongside `_copy_profile()`). It resolves the interdependent `screen` / `window_size` / `window_position` / `viewport` / `no_viewport` / `device_scale_factor` fields from the detected display: crucially, **`headless` defaults to `not has_screen_available`** (headless when no display), and headful-vs-headless picks viewport-mode vs window-fit-mode. It closes with the invariant `assert not (self.headless and self.no_viewport)`.

`_copy_profile()` clones a real Chrome `user_data_dir` into a `browser-use-user-data-dir-*` temp dir (Chrome channels only), skipping transient lock files and raising a clear error on Windows sharing violations. `_ensure_default_extensions_downloaded()` fetches/extracts uBlock Origin Lite, "I still don't care about cookies", and Force Background Tab CRX bundles into `CONFIG.BROWSER_USE_EXTENSIONS_DIR`, parses the `Cr24` CRX header (v2/v3) to reach the ZIP payload, rejects Manifest-V2 extensions, and patches the cookie extension's `background.js` to seed `cookie_whitelist_domains`.

## Logging

[`setup_logging()`](../../browser_use/logging_config.py) is the single entry that configures the root, `browser_use`, `bubus`, and CDP loggers.

### The `RESULT` level

[`addLoggingLevel('RESULT', 35)`](../../browser_use/logging_config.py) registers a custom level numbered **35** (between `WARNING=30` and `ERROR=40`) with a `logging.result(...)` convenience method. It is wrapped in `try/except AttributeError` so re-invocation is idempotent. When `BROWSER_USE_LOGGING_LEVEL=result`, the console is set to level 35 and the format collapses to bare `%(message)s` — the "quiet, only show agent results" mode.

### Name-rewriting formatter

`BrowserUseFormatter.format()` rewrites `record.name` **only** when the effective level is above `DEBUG`: dotted logger names under `browser_use.` are collapsed to a short tag (`Agent`, `BrowserSession`, `tools`, `dom`, else the last dotted segment). DEBUG mode keeps full names. Non-result format is `%(levelname)-8s [%(name)s] %(message)s`.

### Handlers, propagation, third-party silencing

A single `StreamHandler` (to `stream or sys.stderr`) is attached to root, `browser_use`, and `bubus`; the latter two set `propagate = False` to avoid double emission. Optional `debug_log_file` / `info_log_file` add timestamped `FileHandler`s. The root level becomes `DEBUG` whenever a debug file is configured, regardless of console level.

CDP logging is delegated to `cdp_use.logging.setup_cdp_logging(level=..., stream=..., format_string=...)` using `CONFIG.CDP_LOGGING_LEVEL` (default `WARNING`), with a manual fallback if the import is absent. Finally a fixed `third_party_loggers` list (`httpx`, `openai`, `anthropic._base_client`, `urllib3`, `asyncio`, `PIL.PngImagePlugin`, `trafilatura`, general `websockets`, …) is forced to `ERROR` with `propagate = False`.

**Gotcha:** if the root logger already `hasHandlers()` and `force_setup` is False, `setup_logging` returns early without touching anything — so import order matters, and callers that need a specific config (the MCP server) pass `force_setup=True`.

### Log pipes (opt-in)

`FIFOHandler` + `setup_log_pipes(session_id)` create named pipes under `<tmp>/buagent.<suffix>/{agent,cdp,events}.pipe` for non-blocking `tail -f` streaming of the agent/CDP/event-bus logger families. Writes are `O_NONBLOCK`; if no reader is attached the message is dropped.

## Process bootstrap (`browser_use/__init__.py`)

Importing the package has three side effects, in order ([`__init__.py`](../../browser_use/__init__.py)):

1. **Auto logging** — unless `BROWSER_USE_SETUP_LOGGING == 'false'`, it calls `setup_logging(debug_log_file=..., info_log_file=...)` at import time. The MCP server and CLI `--mcp` path set that env var to `false` first to suppress stdout noise on the stdio transport.
2. **asyncio `__del__` monkeypatch** — it wraps `asyncio.base_subprocess.BaseSubprocessTransport.__del__` with `_patched_del`, which no-ops when `self._loop.is_closed()` and swallows the `RuntimeError: Event loop is closed` that CPython otherwise prints from subprocess finalizers during interpreter/loop teardown. Pure cosmetic-noise suppression, but it mutates a stdlib class globally for the whole process.
3. **PEP 562 lazy imports** — a module-level `__getattr__(name)` backed by the `_LAZY_IMPORTS` table maps public symbols (`Agent`, `BrowserSession`/`Browser`, `BrowserProfile`, `Tools`/`Controller`, `DomService`, all `Chat*` providers, `models`, `sandbox`) to `(module_path, attr_name)` pairs, `import_module`-ing on first access and caching the result into `globals()`. This keeps `import browser_use` cheap — the comments note `agent.views` alone costs > 1 s — deferring the heavy provider/agent trees until actually referenced. A `TYPE_CHECKING` block re-declares the same names as real imports so type checkers and linters resolve them.

## CLI dispatch (`cli.py`)

The `[project.scripts]` entry points `browser-use`, `browseruse`, `bu`, `browser` all bind to [`cli.py:main`](../../browser_use/cli.py) (`browser-use-tui` → deprecated shim `browser_use_tui_main`). `main()` records a start time, computes a `(mode, command)` context, and wraps [`_dispatch(args)`](../../browser_use/cli.py) in try/except that always emits a redacted `CLITelemetryEvent` (task text replaced with `[redacted]`, only its length kept) via `ProductTelemetry`.

`_dispatch` routes by inspecting `sys.argv[1:]`:

- `--mcp` → `_run_mcp_server()`: forces `BROWSER_USE_LOGGING_LEVEL=critical`, `BROWSER_USE_SETUP_LOGGING=false`, `logging.disable(CRITICAL)`, then `asyncio.run(mcp_main())` from [`mcp/server.py`](../../browser_use/mcp/server.py). See [`07-mcp-integration.md`](07-mcp-integration.md).
- `install` → `uvx playwright install chromium` (`--with-deps` on Linux, `--no-shell`).
- `init` or `--template`/`-t` → `browser_use.init_cmd.main` (template scaffolding fetched from the GitHub template-library).
- `skill` → `browser_use.skills.install.handle`.
- **default (the "run" path)** → `_run_browser_harness()`: sets `BH_CLIENT=browser-use-cli`, monkeypatches the external `browser_harness` package's help/usage/auth/telemetry text to rebrand "Browser Harness" → "Browser Use", then calls `run.main()`. The real agent execution therefore lives in the separate `browser_harness` package / native binary; `cli.py` is a thin branding + telemetry + dispatch wrapper. Cross-ref [`11-beta-rust-bridge.md`](11-beta-rust-bridge.md).

`_read_harness_task()` extracts the task from `-c`/`--code` or piped stdin so it can be length-counted for telemetry without logging its contents.

## Rust port notes

- **Config layering → collapse it.** The three-source scheme is historical baggage. A Rust rewrite wants one typed struct built by `figment` or `config` (env + `.env` via `dotenvy` + optional `config.json`), with an explicit precedence order instead of `__getattr__`-by-name. The destructive `load_and_migrate_config` reset maps cleanly to a `serde` `#[serde(untagged)]` enum with a fallback arm; keep the uuid7-keyed tables (`uuid` crate) but consider real field migration rather than reset.
- **Live-env re-reads** (`OldConfig` reading `os.getenv` per access) are a Python affordance that rarely survives a port; snapshot config once at startup, expose an explicit reload if genuinely needed.
- **Launch-arg synthesis** ports almost verbatim — it is string list assembly + dedup. The `--disable-features` merge and dict round-trip are trivial with `IndexMap`. Display detection needs platform crates (`core-graphics`/`display-info`), and the CRX header parse (`Cr24` magic, v2/v3 offsets) is a few `byteorder` reads over a `zip` payload.
- **Logging** maps to `tracing` + `tracing-subscriber`; the `RESULT`-at-35 custom level has no direct `tracing` analogue (levels are a fixed enum) — model it as a target/filter or a dedicated span, not a numeric level. The name-rewriting formatter becomes a custom `FormatEvent`.
- **The bootstrap side effects don't port.** PEP-562 lazy imports solve a Python import-cost problem that doesn't exist in a compiled binary; the `__del__` monkeypatch addresses a CPython event-loop-teardown wart with no Rust equivalent. Both are pure dead weight in a port.
- **CLI dispatch** → `clap` subcommands. Note the default path already delegates to a native `browser_harness` binary, so the Rust CLI and the Rust core are arguably the same target; `cli.py` is mostly a shim to preserve today's branding.
