# User guide

browser-use lets an LLM drive a real Chromium browser over the Chrome DevTools
Protocol (CDP) to complete web tasks.

## Install

```bash
uv tool install 'browser-use[cli]'   # persistent `browser-use` binary on PATH
# or, for library use inside a project:
uv add browser-use
```

Provision the browser once (Chromium is downloaded, not bundled):

```bash
browser-use install     # -> playwright install chromium (+ --with-deps on Linux)
browser-use doctor      # diagnose the browser/daemon setup
```

## Minimal library usage

```python
import asyncio
from browser_use import Agent
from browser_use.llm.openai.chat import ChatOpenAI

async def main():
    agent = Agent(task="Find the price of the top result for 'usb-c cable'",
                  llm=ChatOpenAI(model="gpt-4.1-mini"))
    await agent.run()

asyncio.run(main())
```

## CLI / MCP

- `browser-use` — interactive CLI / TUI.
- `browser-use --mcp` — run as an MCP **server** exposing 16 browser tools to an
  MCP client. See [MCP server multi-agent setup](../tools/mcp-multi-agent-setup.md).

## Headless servers (Ubuntu 24.04 note)

On a headless host, set `BROWSER_USE_HEADLESS=true`. Ubuntu 23.10+/24.04 restrict
unprivileged user namespaces (`apparmor_restrict_unprivileged_userns=1`), which
makes Chrome crash unless it launches with `--no-sandbox`; set
`chromium_sandbox: false` in `~/.config/browseruse/config.json` (or run in
Docker, which browser-use auto-detects). See
[learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md](../../learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md)
for the related LLM-gateway gotcha.
