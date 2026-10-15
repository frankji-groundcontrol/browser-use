# Fixes from the MBP-local WIP review (2026-10-15)

## Table of Contents

- [What changed](#what-changed)
- [Behaviour you can observe](#behaviour-you-can-observe)
- [Verification](#verification)
- [Not changed](#not-changed)

## What changed

The three-slice WIP landed in `3c6a553e7` (DOM-click for off-screen nodes,
Flutter coordinate typing, live form state) was reviewed before it was pushed.
This change fixes every finding. Per-finding evidence and the triage of the 18
nits are in [the review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md).

| Area | Change |
| --- | --- |
| `bu-cdp/src/dom.rs` | Autocomplete redaction tokenises the attribute; adds `current-password`/`new-password` and password-like `name`/`id`; fails closed on a node with no attribute record; caps live `value` at 200 chars; toggle detection is case-insensitive |
| `bu-cdp/src/geometry.rs` | `prefers_dom_click` tests the click point against the viewport, so a partly scrolled tall element keeps its trusted mouse event |
| `bu-cdp/src/keys.rs` (new) | Pure key planning: editing commands for accelerators, `text` for printable keys and Enter, virtual key codes, canonical modifier names. Replaces the free helpers that were in `lib.rs` |
| `bu-cdp/src/lib.rs` | DOM click dispatches an event on SVG/MathML nodes, reports the real exception, and refuses a node that is no longer interactive; paste restores the clipboard, scopes and resets the clipboard permission, and never wipes the field when the clipboard write fails |
| `bu-mcp/src/lib.rs` | `browser_type` rejects an index combined with coordinates |
| `bu-actor` tests | The fail-closed policy test drives the three new mutating commands |
| Docs | Generated `SKILL.md` reverted and its note moved to `docs/usage/`; tool count 19 → 21 in the live docs; changelog claims relabelled; board rows added |

## Behaviour you can observe

- `browser_get_state` no longer reports the live value of a card, one-time-code
  or password-autocomplete field when `autocomplete` is a token list such as
  `billing cc-number`.
- `browser_press_key` with `Meta+a`, `Control+v` and similar now performs the
  shortcut instead of reporting success and doing nothing; Enter submits a form;
  Home, End, Delete, PageUp/PageDown and function keys work.
- A failed `browser_paste_at_coordinates` leaves the field's text as it was.
- `browser_type` with both `index` and coordinates is an `INVALID_PARAMS` error.

## Verification

Run from `rust/` with the ambient `BROWSER_USE_*` variables unset:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features live-chrome -- -D warnings
cargo test --features live-chrome -- --test-threads=1
```

All four commands exit 0 on this host: **200 tests passed, 0 failed** (174 before
this pass). No other host ran them.

Each fix was also checked by mutation: the pre-fix code fails the new test for the
stated reason, the fix passes. The skill-sync CI job still exits 1 here, because
it compares against upstream `browser-harness` `main`; see the review findings.

## Not changed

Nothing was deployed or installed on any host, MBP-local's tree is untouched, and
the live check of DOM-click on a real Flutter page is still open.

[Review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md) ·
[Changelog index](README.md)
