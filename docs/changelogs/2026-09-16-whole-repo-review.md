# Whole-repository adversarial review

The repository at `823f8782fa97f8310c4158e87d668d90b9355672` was reviewed across
Python, Rust, browser/CDP, DOM, agent and beta flows, MCP, providers,
integrations, sandbox, CLI, packaging, CI, Docker, deployment, docs and tests.
No implementation code changed.

The detailed, evidence-backed result is [the review issue record](../issues/2026-09-16-whole-repo-review-findings.md), driven by the
[tracked plan](../plans/2026-09-16-whole-repo-review/2026-09-16-whole-repo-review.md).
The most urgent work is to enforce URL policy before Rust actions, repair
full-URL host matching, stop destructive config recovery, minimize telemetry and
domain-scope cookie injection, fix setup/publish workflows, and make the Rust
feature test matrix reliable. Live Rust MCP attachment through both HTTP and raw
WebSocket endpoints was proven against isolated Chrome; Docker, real providers,
and beta process-death behavior remain explicitly blocked or unverified.
