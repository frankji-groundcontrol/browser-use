# Usage

Guides grouped by audience and by tool.

## Audiences

- [library/](library/README.md) — the upstream library handbook: quickstart,
  `Agent`, `Browser`, tools, production, and telemetry. Extracted from
  `AGENTS.md` on 2026-08-26 so the router files stay thin.
- [users/](users/README.md) — running browser-use as a library or CLI to drive
  a browser with an LLM.
- [developers/](developers/README.md) — building on, testing, and extending the
  codebase, plus [local setup from source](developers/local-setup.md).

## Tools

- [tools/](tools/README.md) — reference for the shipped tools.
  - [MCP server multi-agent setup](tools/mcp-multi-agent-setup.md) — expose
    browser-use's MCP tools to external coding agents (Claude Code, Codex,
    OpenCode, Hermes, Grok, Qoder, Kimi Code), including a launcher for gateways
    that block the OpenAI SDK.
  - [Sessions, logging in, and the clipboard](tools/browser-sessions-and-login.md)
    — headful vs headless, persistent profiles, attaching to your own Chrome,
    and what the transient default actually guarantees.

See also the top-level [`examples/`](../../examples) directory for runnable
scripts.
