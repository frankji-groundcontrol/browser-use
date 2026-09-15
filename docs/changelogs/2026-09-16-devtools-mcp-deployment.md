# 2026-09-16 — DevTools MCP verification and deployment

The Rust MCP binary was release-built, probed through a sequential MCP
`initialize` → `tools/list` exchange, and returned 19 tools. The browser-use MCP
tools are available in the active Codex session. The Rust browser session can
attach to user-owned Chrome through `BROWSER_USE_CDP_URL`; no local DevTools
listener was present during this check, so live attachment remains pending.

Workspace tests, both Clippy configurations, the release build, formatting, and
the MCP protocol probe passed. The `live-chrome` matrix compiled but two actor
tests failed because this host has no launchable Chromium. The release binary
was installed on local, MBP2, and franky-frank after the commit was pushed.

Related: [deployment plan](../plans/2026-09-16-devtools-deploy/2026-09-16-devtools-deploy.md),
[DevTools follow-up](../issues/2026-09-16-current-chrome-devtools-endpoint.md).

Follow-up fix: HTTP CDP endpoints now resolve `/json/version` to the browser
WebSocket before calling `chromiumoxide`. A live probe against Chrome 149 on
port 9230 completed `initialize` and `browser_navigate` successfully.
