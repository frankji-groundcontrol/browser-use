# Harden all retained review findings

Date: 2026-09-16. Owner: Codex. Status: complete.

## Outcome

Fix every retained finding in
docs/issues/2026-09-16-whole-repo-review-findings.md, including lower-confidence
correctness and reliability candidates. Every item must end as fixed and proven,
or concretely blocked with a reason.

## Constraints

- Start from docs/HANDOFF.md and preserve the fork's deployment and documentation
  guardrails.
- Production code changes are allowed for this goal. Never read or print
  .env/.env.local; redact secrets and private endpoints.
- Use failing tests or minimal reproductions before fixes for security, data-loss,
  protocol, lifecycle, and concurrency behavior.
- Keep the diff surgical, run the smallest end-to-end path first, then broaden.
- Remove temporary fixtures/scripts. Update this plan, the tracker, changelog,
  learning, issue records, indexes, and handoff at accepted milestones.

## Work packages

| Package | Owner | Scope | Exit evidence |
| --- | --- | --- | --- |
| Python security/data boundaries | python_static_review | SEC-01, SEC-03, SEC-04, SEC-05 and related policy footguns | Focused regression tests plus Python security/CI gates |
| Python browser/DOM/lifecycle | python_static_review/python_agent_dom | click geometry, drag state, concurrent attach, watchdog cleanup/keep-alive/crash target, frame fanout | Focused tests and browser integration evidence |
| Python agent/beta bridge | python_static_review/python_agent_agent | async callbacks, beta timeouts/cleanup/cancellation, RPC edge cases | Focused tests and bridge matrix or explicit block |
| Rust CDP/actor/parity | rust_static_review | RST-01–05, placeholders, feature/provider behavior, live-test determinism | Rust unit/feature/live tests and parity checks |
| Operations/release | ops_contract_review | REL-01–03, setup/publish/Docker and reproducibility | Shell/workflow checks and available build evidence |
| Integration coordinator | Codex | cross-package tests, MCP/Chrome/provider probes, findings synthesis | Full verification matrix and handoff |

## Checklist

- [x] Baseline and ownership synchronized.
- [x] Focused red tests/reproductions added for each package.
- [x] Python security, browser, agent, and operations fixes integrated.
- [x] Rust fixes and parity work integrated.
- [x] Deterministic and end-to-end verification complete.
- [x] Independent regression/false-positive review complete.
- [x] Documentation closure complete and every finding disposition updated.

## Evidence summary

Python formatting, Ruff, Pyright, focused regressions, package build, release contracts, and the full `tests/ci` matrix are the required gates. Rust formatting, Clippy, default tests, env-isolated all-feature tests, live CDP/MCP tests, and provider fixtures are required Rust gates. Docker daemon execution and real external provider credentials are unavailable locally and remain explicitly blocked in the issue record.
