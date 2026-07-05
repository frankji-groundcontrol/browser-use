# browser-use documentation map

This `docs/` tree keeps operating knowledge modular so the router files
([`CLAUDE.md`](../CLAUDE.md), [`AGENTS.md`](../AGENTS.md)) stay thin. It was
organized on the `franky` branch following the `clean-repo-org` practice.

## Sections

- [architecture/](architecture/index.md) — how the library is built: the
  event-driven `BrowserSession`, CDP integration, DOM pipeline, tools registry,
  MCP server, agent loop, and LLM abstraction.
- [usage/](usage/README.md) — guides for end users, developers, and the
  shipped tools (including the MCP-server multi-agent setup).
- [issues/](issues/README.md) — concrete implementation issue records.
- [learning/](learning/README.md) — reusable lessons captured from tasks.
- [plans/](plans/README.md) — dated, living task plans.
- [practices/](practices/README.md) — reusable setup and implementation methods.

## Repo entry points

- [`README.md`](../README.md) — project overview and quick start.
- [`CLAUDE.md`](../CLAUDE.md) — Claude Code router + build/test commands.
- [`AGENTS.md`](../AGENTS.md) — general coding-agent contract.
- [`pyproject.toml`](../pyproject.toml) — package metadata, deps, entry points.
