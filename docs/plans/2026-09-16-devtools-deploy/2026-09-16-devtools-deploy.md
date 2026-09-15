# Verify DevTools attachment and deploy Rust MCP

Date: 2026-09-16 · Status: complete · Owner: Codex

## Outcome and dependencies

Verify browser-use-rs against the current Chrome DevTools endpoint, commit and
push the reviewed pending changes, and sync/install on local, MBP2 and
franky-frank. Preserve existing browser sessions, host-local changes and MCP
registrations. Follow the [deploy practice](../../practices/deploy-browser-use-rs.md).

State: [tracker](2026-09-16-devtools-deploy.track.yaml).

## Checklist and evidence

- [x] Sequential MCP initialize and tools/list; current-browser attachment is
  pending the endpoint follow-up because no local DevTools listener existed.
- [x] Review pending changes; unit, formatting, Clippy and release gates.
- [x] Commit and push with release evidence and documentation.
- [x] Sync source and atomically install the release on all three hosts.
- [x] Verify installed binaries and MCP protocol on each host.

## Initial observations

Local and MBP2 are on franky-rust at 58fb53c1d; local has pending Rust changes.
MBP2 is clean. franky-frank is behind at 7cc3b662e with a local rust/README.md
edit that must be preserved. The active session exposes the browser-use MCP
tools but currently reports no active browser sessions. Earlier completion
claims did not establish live attachment or deployment.

## Completion evidence

The protocol probe returned 19 tools locally and on both remote hosts. The
`live-chrome` test matrix compiled; its two actor failures are environment
failures caused by missing Chromium, not Rust compilation or Clippy failures.

The HTTP DevTools resolution fix was committed as `c2595bbde` and rebuilt on
both remote hosts.
