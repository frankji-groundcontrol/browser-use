# `browser-use-rs` exits silently and successfully when given no arguments

Date: 2026-08-27 · Scope: `crates/bu-core/src/main.rs`

## Problem

The binary only recognizes `--mcp`. Anything else — no arguments, `--help`,
`--version`, a typo — falls through to `Ok(())`:

```
$ browser-use-rs           ; echo $?      # no output, exit 0
$ browser-use-rs --help    ; echo $?      # no output, exit 0
$ browser-use-rs --mpc     ; echo $?      # no output, exit 0  (typo)
```

`main.rs` in full is 19 lines, and the dispatch is one `if`:

```rust
if std::env::args().any(|arg| arg == "--mcp") {
    bu_mcp::run_stdio_server().await?;
}
Ok(())
```

Three ways this misleads:

- Someone running it by hand to check the install gets **no output and a success
  exit**, which reads as "ran fine" rather than "did nothing".
- `--help` and `--version` are the two flags a person tries first on an unknown
  binary, and both are silently swallowed.
- A **typo in an agent's MCP config** (`--mpc`) produces a process that starts,
  writes nothing to stdout, and exits 0. The MCP host sees the connection close
  with no error to report.

The last one matters most: it converts a one-character config typo into a silent
failure with nothing to grep for.

## Why it is like this

Not an oversight so much as unfinished scope. The Rust port is deliberately
MCP-only — from the rewrite plan:

> MVP is scoped to the MCP server (the surface the coding agents use) so the
> first build can replace the current install; the full agent loop is Phase 3.

So there is no CLI to speak of, and argument handling never needed to exist. The
gap only shows when a human, rather than an agent, invokes the binary.

## Impact

Cosmetic in normal operation — every real caller passes `--mcp`, and all seven
agents on both hosts are configured correctly and verified connected. This costs
nothing until someone debugs by hand or mistypes a flag.

## Fix

Small and self-contained:

- `--mcp` → run the stdio server (unchanged).
- `--help` → usage text naming `--mcp` and the `BROWSER_USE_LLM_*` variables.
- `--version` → the crate version.
- Anything else, including no arguments → usage on stderr, **exit non-zero**.

Acceptance: `browser-use-rs` with no args exits non-zero and prints usage;
`--mpc` does the same rather than exiting 0; `--mcp` behaviour is byte-identical
to today, verified by the existing `tools/list` probe still returning 19 tools.

## Related

The Python entry point does have a real CLI, so this divergence is specific to
the Rust port's MCP-only scope: [architecture 12](../architecture/12-rust-implementation.md),
[rust-rewrite progress](../plans/2026-07-05-rust-rewrite/progress.md).
