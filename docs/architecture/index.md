# Architecture

browser-use is an async Python library that lets LLMs drive a real Chromium
browser over the Chrome DevTools Protocol (CDP). It follows an **event-driven**
design: a `BrowserSession` owns the CDP connection and coordinates a set of
watchdog services over a `bubus` event bus.

> This index is the soft index for architecture docs. Detailed per-concern files
> are added alongside it (see the subsystem table). Keep this file current when
> source layout or major workflows change; keep [`CLAUDE.md`](../../CLAUDE.md)
> and [`AGENTS.md`](../../AGENTS.md) as thin routers that link here.

## Core components

| Subsystem | Source | Role |
| --- | --- | --- |
| Agent | [`browser_use/agent/`](../../browser_use/agent/) | Orchestrator; the LLM-driven perceive→decide→act loop, message manager, system prompts. |
| BrowserSession | [`browser_use/browser/`](../../browser_use/browser/) | Browser lifecycle, CDP connection, and the watchdog event bus. |
| Watchdogs | [`browser_use/browser/`](../../browser_use/browser/) | Downloads, popups, security, DOM, about:blank — reactive services on the bus. |
| DOM service | [`browser_use/dom/`](../../browser_use/dom/) | Extracts/serializes the DOM + accessibility tree; builds the interactive-element selector map. |
| Tools | [`browser_use/tools/`](../../browser_use/tools/) | Action registry mapping decisions to browser operations; `ActionResult`. |
| MCP | [`browser_use/mcp/`](../../browser_use/mcp/) | MCP server (16 tools over stdio) + client; the external-agent integration surface. |
| LLM | [`browser_use/llm/`](../../browser_use/llm/) | Provider abstraction (`BaseChatModel`) over OpenAI, Anthropic, Google, Groq, AWS, Azure, … |
| Actor | [`browser_use/actor/`](../../browser_use/actor/) | Lower-level action execution layer. |
| Support | [`config.py`](../../browser_use/config.py), [`cli.py`](../../browser_use/cli.py), `tokens/`, `sync/`, `filesystem/`, `telemetry/`, `sandbox/` | Config, CLI entry points, token accounting, cloud sync, file I/O, telemetry, sandboxing. |

## Subsystem documents

Read these for the real, source-grounded detail (each cites specific files):

- [00 — System Overview & Layering](00-system-overview.md)
- [01 — CDP Transport & Session Management](01-cdp-transport-and-session-manager.md)
- [02 — The bubus Event Bus & Watchdog Services](02-event-bus-and-watchdogs.md)
- [03 — DOM Perception: Three-Tree Fusion & Serialization](03-dom-perception-pipeline.md)
- [04 — Tools, Action Registry & ActionResult](04-tools-and-action-registry.md)
- [05 — Agent Orchestrator & LLM Action Loop](05-agent-control-loop.md)
- [06 — LLM Provider Abstraction & Structured Output](06-llm-provider-abstraction.md)
- [07 — MCP Server & Client Integration](07-mcp-integration.md)
- [08 — Actor: Imperative CDP Scripting Facade](08-actor-scripting-api.md)
- [09 — Configuration, Logging & Process Bootstrap](09-configuration-logging-bootstrap.md)
- [10 — Cross-Cutting Services](10-cross-cutting-services.md)
- [11 — The Beta Rust Bridge & JSON-RPC Contract](11-beta-rust-bridge.md)

For the forward-looking Rust rewrite, see the plan at
[docs/plans/2026-07-05-rust-rewrite/](../plans/2026-07-05-rust-rewrite/index.md).

## Key patterns

- **Event bus (`bubus`)** — `BrowserSession` dispatches events
  (`BrowserStartEvent`, `NavigateEvent`, `BrowserStateRequestEvent`, …); watchdogs
  subscribe and react. This decouples browser concerns into isolated services.
- **CDP via `cdp-use`** — a thin typed wrapper around the DevTools protocol;
  all CDP client/session management lives in `browser_use/browser/session.py`
  (`cdp_client.send.Domain.method(...)`, `cdp_client.register.Domain.event(...)`).
- **Service / Views / Events split** — each major component keeps logic in
  `service.py`, pydantic models in `views.py`, event types in `events.py`.
- **Dynamic action models** — the tools registry builds pydantic action models
  at runtime so the LLM's structured output maps directly onto browser actions.

## Entry points

- `browser-use` (CLI/TUI) and `browser-use --mcp` (MCP server) →
  `browser_use.cli:main`.
- Library: `from browser_use import Agent, BrowserSession, Tools`.

## MCP tool surface

The MCP server exposes 16 tools (14 low-level browser primitives + 2 LLM-backed).
See [usage/tools/mcp-multi-agent-setup.md](../usage/tools/mcp-multi-agent-setup.md).
