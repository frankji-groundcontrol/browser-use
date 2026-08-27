# Library guide

Upstream's browser-use library documentation — how to install it, drive the
`Agent`, configure the `Browser`, and extend the tool registry.

This content previously lived inline inside [`AGENTS.md`](../../../AGENTS.md) as
a ~985-line embedded block. It is kept here so the router files stay thin; read
the page you need rather than loading the whole handbook.

## Pages

| Page | Covers |
| --- | --- |
| [Quickstart](quickstart.md) | Install, pick an LLM, run a first agent |
| [Agent](agent.md) | `Agent` basics, every parameter, output format, prompting guide, supported models |
| [Browser](browser.md) | `Browser` basics, every parameter, real local Chrome, remote/cloud browsers |
| [Tools](tools.md) | Action registry basics, adding and removing tools, the built-in set, tool responses |
| [Going to production](production.md) | Deployment, proxies for stealth, cookie sync |
| [Help and telemetry](support.md) | Where to get help; what telemetry collects and how to disable it |

Contributing to the library itself? See the
[developer guide](../developers/README.md) and
[local setup from source](../developers/local-setup.md).

## Scope

These pages track upstream. This fork's own operating knowledge — the deployed
Rust MCP server, its multi-agent setup, and deploy practices — lives under
[usage/tools/](../tools/README.md), [architecture/](../../architecture/index.md),
and [practices/](../../practices/README.md).
