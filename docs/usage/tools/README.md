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

The MCP server exposes 16 tools: 14 low-level browser primitives that need no
LLM key (the calling agent is the brain) and 2 LLM-backed tools
(`browser_extract_content`, `retry_with_browser_use_agent`) that use the
server's own OpenAI-compatible (or AWS Bedrock) model.
