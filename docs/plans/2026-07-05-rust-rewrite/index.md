# Plan: Rust rewrite (2026-07-05)

Status: **active** · Branch: `franky-rust` · Method: **TDD**

Re-implement browser-use in Rust, delivered as a Cargo workspace, developed
test-first, and driven to completion by codex under orchestration. This folder is
the source of truth for the effort; keep [progress.md](progress.md) current.

## Goal and guiding constraint

The thing we must ultimately **install and replace** is the surface the coding
agents actually use: the `browser-use --mcp` **server** (16 tools over stdio,
driving Chromium via CDP). Therefore the plan is ordered so the **first
installable artifact is a Rust MCP server** that is a drop-in replacement for the
14 low-level tools, then the 2 LLM-backed tools, then the full autonomous agent.
Everything else (all 17 LLM providers, every watchdog, cloud sync) is later
parity work.

## Prior art (important)

`browser_use/beta/` already drives a **native Rust binary** (`browser-use-terminal`
/ the `browser-use-core` wheel) over newline-framed **JSON-RPC 2.0**. Its wire
contract (`runtime.ping`, `agent.run_task`, `agent.run`, the `agent.event`
notification stream, the `history{events,usage,success,errors}` result shape) is
the **authoritative external spec** and doubles as a conformance test suite. See
[architecture/11-beta-rust-bridge.md](../../architecture/11-beta-rust-bridge.md).
We do not have that crate's source; `franky-rust` is a fresh implementation in
this repo, but it should honor that contract where practical.

## Child documents

- [architecture.md](architecture.md) — target Cargo workspace and per-crate design.
- [porting-map.md](porting-map.md) — Python module → Rust crate mapping, scope
  decisions (port / defer / sidecar), and per-area risk.
- [plan.md](plan.md) — the phased, TDD implementation plan with exit criteria.
- [tdd-strategy.md](tdd-strategy.md) — golden-file + live-Chrome test approach and
  fixture capture.
- [progress.md](progress.md) — living status log (updated every task).

## Non-goals (v1)

- Bit-for-bit parity with every Python provider/watchdog/cloud feature.
- Porting the `sandbox/` cloudpickle+AST remote executor (no Rust analog — keep as
  a Python sidecar or redesign as serializable task descriptors).
- The `beta/` event→history reconstruction layer (dead weight in a native build).

## Source of truth for architecture

The Python architecture this plan ports from is documented in
[docs/architecture/](../../architecture/index.md). Read it before implementing a
crate.
