# Whole-repository hardening and proof

Date: 2026-09-16

Implemented the retained security, reliability, Rust parity, DevTools, DOM,
agent, beta, release, and test-matrix findings from the whole-repository review.
Python now fails safe on malformed config, scopes cookies, minimizes telemetry,
and enforces URL authority boundaries. Rust now resolves and attaches DevTools
targets safely, guards mutating actions, bounds browser shutdown, and exposes
provider completion/stream contracts with deterministic fixtures. Setup,
release, and fast Docker workflows use one verified environment/version/lock
chain.

Evidence: Python Ruff, format, Pyright, focused regressions, package build,
release-contract tests, and the full CI suite; Rust format, Clippy, default and
env-isolated all-feature workspace tests, live CDP/MCP tests, and 61 all-feature
LLM tests. Docker daemon execution and real provider credentials were unavailable
and remain explicitly blocked in the issue record.

Related: [hardening plan](../plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md), [finding resolutions](../issues/2026-09-16-whole-repo-review-findings.md), [learning record](../learning/2026-09-16-hardening-review-findings.md).
