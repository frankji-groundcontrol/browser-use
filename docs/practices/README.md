# Practice records

Reusable setup methods, module patterns, command sequences, and operational
techniques worth repeating. One file (or folder) per practice.

## Index

- [Deploying `browser-use-rs`](deploy-browser-use-rs.md) — build → fresh-inode
  install + codesign → four-step verification ladder (exec, protocol probe,
  registration, sequential live smoke with visual screenshots).
- [Running the Python CI suite on this machine](running-python-ci.md) — the
  proxy-aware pytest invocation, hang diagnosis via `--timeout-method=thread`,
  and order-pollution triage rules. Baseline: 1024 passed / 0 failed.
- [Syncing upstream and porting fixes into the Rust rewrite](upstream-sync-and-port.md)
  — merge, map-and-adversarially-verify candidates against the real Rust
  source, port test-first, prove each regression lock by reverting the fix.
- MCP server multi-agent setup — see the usage guide
  [usage/tools/mcp-multi-agent-setup.md](../usage/tools/mcp-multi-agent-setup.md),
  which doubles as the reusable practice for wiring `browser-use --mcp` into
  multiple coding agents behind a gateway.
