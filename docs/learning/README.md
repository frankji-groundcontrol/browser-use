# Learning records

- [2026-10-25 — Verify Rust port contracts at every consumer](2026-10-25-verify-rust-port-contracts.md) — consumer projections, shared protocol serializers, launch behavior, and fork merge checks.

- [2026-10-16 — Reproduce on the object production uses, and assert the precondition](2026-10-16-reproduce-on-the-object-production-uses.md)
  — a harness that opened a fresh tab never saw the unfocused initial tab; two
  tests asserting an eventual effect stayed green with the fix removed.
- [2026-10-15 — A pinned execution context races navigation, and load makes it lose](2026-10-15-pinned-execution-context-races-navigation.md)
  — a cached `contextId` is destroyed mid-navigation; a silent test under load is
  starvation until a stack sample says otherwise, so bound it with a timeout.
- [2026-10-15 — A matching HEAD hides a dirty tree on another host](2026-10-15-matching-head-hides-a-dirty-tree.md)
  — check `git status` on the remote before calling a sync a no-op; move
  uncommitted work with `git diff --binary` plus a tar, proven by per-file hashes.
- [2026-09-16 — Hardening lessons from the whole-repository review](2026-09-16-hardening-review-findings.md)
  — parsed URL boundaries, keep-alive event-bus rebind, blocking HTTP fixtures, and non-interactive cargo PATH.
- [2026-09-16 — Lessons from the whole-repository review](2026-09-16-whole-repo-review.md)
  — default-feature green tests, semantic shell proofs, documentation drift,
  and raw data hidden behind “anonymized” telemetry.

Reusable lessons captured from tasks: what was learned, the evidence, its scope,
and when to apply it again. One dated file (or dated folder) per lesson.

## Index

- [2026-07-05 — OpenAI-compatible gateways can block the SDK by User-Agent](2026-07-05-openai-gateway-sdk-fingerprint-block.md)
  — `/v1` path, model family, and User-Agent WAF gotchas for the MCP server's
  LLM-backed tools behind a proxy gateway.
- [2026-08-26 — In a fork, "keep the router thin" applies only to the region the fork owns](2026-08-26-fork-router-editable-region.md)
  — a 1026-line `AGENTS.md` that is 15 lines diverged from upstream; measure the
  diff against upstream before restructuring any inherited file.
- [2026-08-27 — An integration that works only because the counterparty is lenient is untested](2026-08-27-working-by-accident-is-not-working.md)
  — the "Anthropic" option never spoke Anthropic and worked for months anyway;
  grep for the one header unique to a protocol before believing the label.
- [2026-09-15 — Port source-remote settings at the policy boundary](2026-09-15-rust-port-source-remote.md) — shared policy fields avoid duplicated security guards; chromiumoxide handles HTTP DevTools discovery.
