# Upstream sync and Rust contract ports

Date: 2026-10-25
Status: complete — published to the owner fork and installed locally

The Rust fork has been synchronized through upstream `3df5806a0` (Python
0.13.13), preserving the existing uncommitted focus/Flutter/context fixes.
The configured tracking branch was current; the upstream branch added 361
commits since the common base.

Confirmed Rust changes cover agent live form observations, bounded image
context and data-source privacy, hidden-code extraction, Anthropic image
serialization, native date/time values, disabled dropdown choices, browser
launch configuration and connection recovery. Review regressions also lock
validated-page identity, image URL authority privacy, and CSS declaration
boundaries. The existing 21-tool MCP API is
retained. Broader Python SDK, file-export and cloud-provisioning APIs are
explicitly outside Rust parity claims.

Six inherited merge conflicts preserve both upstream behavior and fork
hardening. The Python lock was refreshed for the merged source; the global
Python CLI is outside this Rust installation task.

See [verification](verification.md), the
[live plan](../../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.md),
[applicability matrix](../../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.upstream-matrix.md),
[lessons](../../learning/2026-10-25-verify-rust-port-contracts.md),
[sync practice](../../practices/upstream-sync-and-port.md), and
[module follow-up](../../issues/2026-10-25-rust-module-size-debt.md).

Implementation/source commit: `0d9e428e8994510484c553c86f58fba290d72d39`. The local signed install answers
21 MCP tools and passed the live smoke. All 243 Rust and 51 focused Python
merge tests passed, along with formatting, both Clippy configurations and the
release build. The final documentation closure is a separate commit; the binary
was built from the implementation commit above.
