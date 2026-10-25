# Rust module size follow-up

Date: 2026-10-25
Status: open, outside the upstream behavior-port scope

The sync preserves several existing mixed-responsibility files. Splitting them
while changing browser semantics would widen review and invalidate independent
worker ownership. Track the structural work separately; it is not a release
blocker for the tested behavior ports.

| Current file | Proposed structure |
| --- | --- |
| `rust/crates/bu-cdp/src/lib.rs` | Thin public exports; session/launch ownership, page state/screenshots, and native input in focused modules; retain existing keys/dom/discovery/security modules |
| `rust/crates/bu-cdp/src/dom.rs` | Snapshot extraction, sensitivity/live-value policy, interaction filtering, and image context as focused modules |
| `rust/crates/bu-mcp/src/tests.rs` | Existing contract helpers with grouped input, DOM, lifecycle, security and LLM test modules |
| `rust/crates/bu-agent/src/lib.rs` | Agent loop plus a sibling live-test module containing the existing local model fixture |
| `rust/crates/bu-dom/src/lib.rs` | HTML attributes/inline style parsing, Markdown extraction/chunking, and tests in focused modules |

Bounded fix prompt: move one responsibility at a time without changing public
signatures, error handling, bounds, security checks or browser ownership. The sync already extracted the native-date helper into `input.rs`. Next,
move remaining keyboard/clipboard and coordinate input helpers out of the CDP root; run the existing unit and serial
live-Chrome contracts on default and live feature configurations. Do not add
new dependencies or redesign the actor in the same change.

See the [sync plan](../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.md)
and [applicability matrix](../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.upstream-matrix.md).
