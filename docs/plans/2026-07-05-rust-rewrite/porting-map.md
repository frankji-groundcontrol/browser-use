# Porting map: Python module → Rust crate

Scope legend: **port** (reimplement in Rust) · **defer** (later parity phase) ·
**sidecar** (keep as a Python process; no clean Rust analog) · **drop** (not
needed in a native build).

| Python module | Rust crate | Scope | Risk | Notes |
| --- | --- | --- | --- | --- |
| `browser/session.py`, `session_manager.py`, `_cdp_timeout.py` | `bu-cdp` + `bu-session` | port | **high** | Single-lock `SessionState`; explicit `ConnState`/`RecoveryState`; 60s per-call timeout. The concurrency invariants are correctness-load-bearing. |
| `browser/watchdogs/*`, `watchdog_base.py`, `events.py` | `bu-bus` + watchdog modules | port (core) / defer (rest) | **high** | Port downloads/security/dom/local-browser first; defer video/HAR/storage. Reflection registration → proc-macro or explicit registry. |
| `dom/*` | `bu-dom` | port | med | **Highest confidence-per-effort.** Pure sync serializer; arena tree; preserve `selector_map == backendNodeId`. |
| `tools/*` | `bu-tools` | port | med | Dynamic action union → `Value` schema + `jsonschema`. Special-param DI → `ActionCtx`. |
| `agent/*` | `bu-agent` | port | med-high | Dynamic `AgentOutput`, 3-slot message manager, loop detection. |
| `llm/*` (openai + compatible) | `bu-llm` | port | med | `async-openai` covers OpenAI-compatible; SchemaOptimizer is pure. |
| `llm/*` (anthropic, google, groq, bedrock, …) | `bu-llm` | defer | med | Hand-roll over `reqwest`; cache_control + forced tool_choice + thinking-block extraction. cfg-feature per provider. |
| `mcp/server.py`, `mcp/client.py` | `bu-mcp` | port | low-med | `rmcp`; server re-exposes `bu-tools`; keep stdout JSON-RPC clean. |
| `actor/*` | `bu-actor` | port | low | Clean, bus-free; fallback ladders (quads→boxModel→JS); keycodes via `phf`. |
| `config.py`, `cli.py`, `logging_config.py`, `__init__.py` | `bu-config` + `bu-core` | port | low | Explicit accessors replace `__getattr__`/PEP-562/monkeypatches. |
| `tokens/` | `bu-llm` (wrapper) | port | low | `TokenTracked<M>` wrapper + pricing table. |
| `filesystem/` | `bu-filesystem` | defer | low | In-memory VFS; needed by the extract/file tools. |
| `sync/` (cloud) | — | defer | low | OAuth2 device-grant + cloud API; parity-phase. |
| `telemetry/` | `bu-telemetry` | defer | low | PostHog; optional, off by default. |
| `skills/`, `integrations/` (Gmail 2FA) | — | defer | low | Remote SDK skills, 2FA. |
| `sandbox/` (cloudpickle + AST) | — | sidecar/drop | n/a | No Rust analog; keep Python sidecar or redesign as serializable task descriptors. |
| `beta/` (Rust bridge, event→history) | — | drop (as impl) / **spec** | n/a | Dead weight natively, but its JSON-RPC contract is the conformance target. |

## Preserve these exact contracts

1. `selector_map` "index" == CDP `backendNodeId` (DOM ↔ tools ↔ agent).
2. `multi_act` before/after URL + focus comparison ordering around each awaited
   action.
3. The beta JSON-RPC method/notification/result shapes
   ([11-beta-rust-bridge.md](../../architecture/11-beta-rust-bridge.md)).
4. MCP stdout is a pure JSON-RPC channel; all logs go to stderr.
