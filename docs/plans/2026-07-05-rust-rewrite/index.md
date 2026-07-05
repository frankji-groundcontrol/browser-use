# Plan: Rust rewrite (2026-07-05)

Status: **draft** — full plan authored in a subsequent step.

Goal: re-implement browser-use in Rust with test-driven development on the
`franky-rust` branch, prioritizing a drop-in replacement for the `browser-use
--mcp` server (the surface used by external coding agents). This index is a
placeholder; the detailed, modular plan (crate/workspace layout, per-subsystem
porting order, TDD strategy, CDP/MCP/DOM design, risks) is written into sibling
files here after the codebase deep-read and an eng review.

Planned child files:

- `plan.md` — phased implementation plan.
- `architecture.md` — target Rust workspace + crate design.
- `tdd-strategy.md` — test-first approach and fixtures.
- `porting-map.md` — Python module → Rust crate mapping and scope decisions.
- `progress.md` — living status log.
