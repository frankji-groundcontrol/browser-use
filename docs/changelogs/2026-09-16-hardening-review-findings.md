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

The completion audit that reopened RST-04 is closed: Bedrock Converse now
serializes tools, streams argument deltas, retries transient failures, bounds
timeouts, and records usage against a local fixture. Keep-alive reconnect
rebinds session handlers after `stop()` replaces the event bus. Cookie injection
rejects single-label suffix domains. `bin/setup.sh` is proven to create and
verify one repository-root environment.

Evidence: Python Ruff, format, Pyright, focused regressions, package build,
release-contract tests, and `tests/ci` 1116 passed / 34 skipped; Rust format,
Clippy, default tests, two agreeing env-isolated all-feature runs (70 LLM tests
with Bedrock), and live CDP/MCP attach/policy tests. Docker daemon execution
and real provider credentials were unavailable and remain explicitly blocked.

Related: [hardening plan](../plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md), [finding resolutions](../issues/2026-09-16-whole-repo-review-findings.md), [learning record](../learning/2026-09-16-hardening-review-findings.md).
