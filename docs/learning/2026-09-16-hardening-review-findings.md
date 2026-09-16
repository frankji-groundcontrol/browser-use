# Hardening lessons from the whole-repository review

Date: 2026-09-16

- URL policy must compare parsed authority fields immediately before an action;
  string prefixes cannot represent host, userinfo, port, and path boundaries.
- Browser shutdown is part of the command contract. A graceful close needs a
  deadline and a kill/reap fallback or the actor can lose its reply channel.
- Feature matrices need isolated configuration. Ambient endpoint variables made
  a live fixture appear to fail until the test explicitly controlled the
  `BROWSER_USE_LLM_*` surface.
- A partial Rust port is safer when architecture docs name the implemented
  boundary and unsupported provider capabilities return explicit errors.

Related: [hardening changelog](../changelogs/2026-09-16-hardening-review-findings.md), [finding resolutions](../issues/2026-09-16-whole-repo-review-findings.md), [hardening plan](../plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md).
