# Whole-repository adversarial review

Date: 2026-09-16. Owner: Codex. Status: complete; unresolved items are recorded
as findings or concrete environment blocks.

## Outcome and constraints

Review the entire tracked repository at `823f8782fa97f8310c4158e87d668d90b9355672`
on `franky-rust`. Produce a coverage map, reproducible findings, explicit limits,
and a remediation sequence. Production source remains unchanged. Do not inspect
environment files or record credentials, private endpoints, or runtime paths.

## Goal prompt

Perform an adversarial, evidence-driven review of every tracked production
surface in this repository: Python library, beta bridge, Rust workspace,
browser/CDP, DOM, watchdogs, agents, tools, MCP, providers, sandbox,
integrations, CLI, packaging, CI, Docker, deployment, documentation, and tests.
Treat existing claims as hypotheses. Inventory contracts and risks first;
trace entry points and trust boundaries; run deterministic checks and real
local-browser/protocol probes; challenge failures and findings once for false
positives. Do not change implementation code. For each finding record its
trigger, exact source location, observed versus expected behavior, reproduction,
confidence, and smallest remediation. Give every area a proven, refuted, or
blocked disposition with a concrete reason; do not infer coverage from a green
aggregate test count. Preserve evidence and end with fixes ordered by impact.

## Proof plan

- [x] Phase 0: baseline commit, manifests, tracked inventory and owners.
- [x] Phase 1: parallel Python, Rust, and operations contract review.
- [x] Phase 2: Python/Rust lint, format, type, tests, builds and protocols.
- [x] Phase 3: real Chromium attach, interactions and ownership checks.
- [x] Phase 4: adversarial URL, malformed protocol and lifecycle checks.
- [x] Phase 5: parity, documentation and deployment-claim audit.
- [x] Phase 6: independent false-positive pass, report and remediation plan.

## Review ownership

| Surface | Reviewer | Contract / principal risk | Evidence method |
| --- | --- | --- | --- |
| Python public API, config, CLI | Python reviewer / Codex | Import and configuration compatibility; persistence | Type/lint/build, CI tests, temporary config repro |
| Python browser, watchdogs, CDP | Python reviewer | Resource ownership, policy and reconnect | Call tracing, live CI browser tests |
| Python DOM and actor | Python reviewer / Codex | Geometry, frames, stale indices | Live browser tests, interaction reproduction |
| Agent, beta bridge | Python reviewer | Ordering, cancellation, framed child RPC | CI tests, static timeout/cleanup traces |
| LLM, tokens, integrations, skills, sandbox, sync, telemetry | Python reviewer / security verifier | Wire contracts, serialization, credential/data boundaries | Provider tests, sentinel checks, trust-boundary audit |
| Python MCP and tools | Codex / Python reviewer | Schemas, framing and recoverable errors | Protocol probes and CI tests |
| Every Rust crate | Rust reviewer / independent verifier | Policy parity, actor ownership, DOM and provider behavior | Workspace/feature checks and real Chrome tests |
| CI, Docker, scripts, manifests, docs, examples and tests | Operations reviewer / independent verifier | Reproducibility, releases, honest claims | Packaging/build checks, shell semantics, workflow and link audit |

## Current evidence

- Inventory: 630 tracked files, including 184 under `browser_use`, 47 under
  `rust`, 111 under `tests`, 124 examples, 85 documentation files, and 18 GitHub
  files. Generated build products are excluded from source review.
- Python: Pyright passed; format check found two files. Full CI suite is running.
  An initial Ruff command auto-fixed one blank line because repository config
  enables fixes; that edit was immediately restored. Repeat with `--no-fix`.
- Python wheel and source distribution built successfully.
- Rust: formatting and all-feature Clippy passed. Parallel all-feature tests
  failed in the browser actor because this environment could not keep the
  launchable browser actor alive. Record this as an unproven feature gate, not
  a product failure. Focused protocol probes passed.
- Second reviewers are challenging security, operational and Rust findings;
  the resulting accepted, rejected, and blocked claims are in the issue record.

The [durable tracker](2026-09-16-whole-repo-review.track.yaml) owns step state.
