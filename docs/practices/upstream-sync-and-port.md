# Syncing upstream and porting fixes into the Rust rewrite

How the 2026-08-02 sync (131 upstream commits → 7 ported defects) was run;
repeat this shape for future syncs.

## 1. Merge

Measure the current fork footprint before merging; it is no longer limited to
routers and a launcher. The 2026-10-25 merge also touched committed Actor,
session, release-workflow and test hardening, producing six content conflicts.
Preserve dirty-file contents separately, compare incoming paths, and inspect
both committed sides when conflicts occur. Verify with the
[CI recipe](running-python-ci.md); one apparent failure was environmental
(SOCKS proxy), so check env before blaming the merge.

## 2. Map upstream changes onto the Rust port

Diff the Rust-relevant Python areas since the last sync base
(`git diff <base> upstream/main -- browser_use/dom browser_use/browser
browser_use/llm ...`) and, per subsystem, ask: *did a fix land after the point
the Rust port claims parity with?* Two rules that mattered:

- **Read the upstream test, not just the diff** — the test encodes the exact
  contract (e.g. the paint-order scoping test uses the *same* CDP session with
  differing iframe `frame_id`s, which killed the "OOPIF-only, not applicable"
  shortcut).
- **Adversarially verify every candidate against the actual Rust source**
  before porting. Of 12 candidates, 5 were refuted (different mechanism,
  architecture makes the bug impossible, or wrong Python baseline). A false
  "not applicable" is as costly as a false positive — one refuted-then-probed
  claim turned out to be the *worse* iframe-bounds bug.

## 3. Port test-first, and prove the lock

- Write the failing test from the upstream contract *before* the fix; several
  "obvious" ports were wrong on first reasoning and only the RED test showed
  it (980px mobile fallback; 0x0 viewport clear; getContentQuads staleness).
- After GREEN, **revert the fix temporarily and rerun** — a regression lock
  that doesn't fail without its fix is decoration. Both hardening tests here
  were validated that way (stale indices survive; zero LLM requests).
- Mocking budget: the LLM only. `bu-agent`'s `ScriptedLlmServer` already
  captures request bodies — check for an existing seam before building one
  (a redundant `ScriptedLlm` variant got built and reverted this way).

## 4. Close out

Full Rust suite serial (`cargo test --features live-chrome -- --test-threads=1`)
+ `clippy -D warnings` on every feature set touched, update
`plans/2026-07-05-rust-rewrite/progress.md`, then
[deploy](deploy-browser-use-rs.md).

## 5. Verify each consumer and configuration effect

Use the [2026-10-25 lessons](../learning/2026-10-25-verify-rust-port-contracts.md)
and [matrix](../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.upstream-matrix.md):

1. Follow captured data through MCP and autonomous agent projections separately.
2. Apply one independent wire contract to simple, typed and streamed requests.
3. Prove browser launch settings by their browser behavior, not an env-name grep.
4. Keep SDK-only/absent subsystems explicit; preserve the Rust API's scope.
5. Resolve inherited hardening semantically and run retained regressions.
6. Install a fresh signed binary inode, probe it sequentially, then compare the
   installed hash with its own build artifact. A Git update alone is not deploy.

The review boundary also includes the resource returned by a check: use the
validated page throughout an operation rather than looking it up again after a
possible reconnect. For image metadata, extract a parsed URL path rather than
its authority; for inline styles, match declarations rather than substrings.

For publication, verify the owner fork and branch explicitly. If anonymous
HTTPS reads succeed but a non-interactive push cannot obtain credentials, test
the machine's existing SSH access to the same fork. A clone-local transport
change may use that existing authentication; preserve the owner/repository
path and confirm the published branch SHA after the push.
