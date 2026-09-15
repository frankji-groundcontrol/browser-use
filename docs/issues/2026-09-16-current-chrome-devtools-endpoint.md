# Current Chrome DevTools endpoint is not exposed

## Observation

On 2026-09-16 the browser-use MCP tool was available and its Rust server
returned 19 tools, but the local host had no active browser-use session and no
Chrome listener or `DevToolsActivePort` file. The release therefore could not
prove attachment to the user's current Chrome.

## Resolution

Start Chrome with remote debugging and set `BROWSER_USE_CDP_URL` to its local
HTTP DevTools endpoint before launching the MCP server. The Rust attach path
preserves the existing browser and does not close it during session cleanup.

## Exit evidence

Run a sequential MCP probe that calls `initialize`, `tools/list`, and a browser
operation against the attached session. Close only the Rust session and verify
the DevTools endpoint remains reachable.
