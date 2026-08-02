# Tools reference

Reference docs for the tools browser-use ships.

- [mcp-multi-agent-setup.md](mcp-multi-agent-setup.md) — run the deployed
  **`browser-use-rs --mcp`** (the Rust reimplementation;
  [architecture/12](../../architecture/12-rust-implementation.md)) as an MCP
  server and register it with multiple coding agents (Claude Code, Codex,
  OpenCode, Hermes). The Rust client needs no SDK-User-Agent workaround; the
  Python server + its gateway launcher wrapper
  ([`contrib/mcp/mcp-launch.py`](../../../contrib/mcp/mcp-launch.py)) are the
  documented rollback path.

- [browser-sessions-and-login.md](browser-sessions-and-login.md) — logging into
  sites when the browser is headless: persistent profiles
  (`BROWSER_USE_USER_DATA_DIR`), attaching to your own Chrome
  (`BROWSER_USE_CDP_URL`), what "transient session" does and does not
  guarantee, and reading the clipboard.

The Rust MCP server exposes **18 tools**: the 16 whose schemas are
byte-identical to Python's — 14 low-level browser primitives that need no LLM
key (the calling agent is the brain) plus 2 LLM-backed tools
(`browser_extract_content`, `retry_with_browser_use_agent`) using the server's
own OpenAI-compatible (or AWS Bedrock) model — and 2 additions this fork made:
`browser_set_viewport` (responsive checks without losing the session) and
`browser_read_clipboard` (capture what a page's "Copy" button wrote).
