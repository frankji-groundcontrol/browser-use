# Flutter index typing (2026-10-15)

> **Superseded 2026-10-16.** The tap-first workaround described below was removed
> once the cause was measured: the initial tab is not focused, and the fix is
> focus emulation on every page we drive. See
> [page focus and Flutter gaps](2026-10-16-page-focus-and-flutter-gaps.md). The
> record is kept as it was written; the double-tap on buttons and checkboxes and
> the off-screen error it mentions no longer exist.

## Table of Contents

- [What changed](#what-changed)
- [Behaviour you can observe](#behaviour-you-can-observe)
- [Verification](#verification)
- [Not changed](#not-changed)

## What changed

[`browser_type` by index silently dropped text on Flutter Web fields](../issues/2026-10-15-flutter-index-typing-drops-text.md);
coordinates worked. `bu-cdp/src/lib.rs` now detects a Flutter semantics node
(`flt-*` tag, or a `data-semantics-role` attribute) with one `DOM.describeNode`.
For such a node, `type_into_backend_node_id` taps the node's box centre with a
real mouse click before inserting, and `clear_backend_node_id` taps, then clears
with select-all and Backspace instead of resetting the proxy `<input>`'s value.
Every other page takes the old DOM-focus path.

## Behaviour you can observe

- `browser_type` with `index` into a Flutter field reaches the app. Replacing and
  clearing (`text: ""`) work the same way.
- Typing by index into a Flutter node now involves two real taps (one to clear,
  one to type). Any other `flt-*` node passed to `browser_type`, such as a button
  or checkbox, is tapped twice too; the old path only focused it.
- A Flutter node outside the viewport, or under 2×2 px (the accessibility
  placeholder), makes `browser_type` return an error asking for a scroll, instead
  of tapping blind: a missed tap still reports success and the keyboard would
  then edit whichever field was focused before.
- Plain pages are unaffected: a control test asserts no pointer event reaches a
  plain `<input>`.

## Verification

Run from `rust/` with the ambient `BROWSER_USE_*` variables unset:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features live-chrome -- -D warnings
cargo test --features live-chrome -- --test-threads=1
```

Five live tests in `bu-cdp/src/live_tests.rs`. Two drive a page that mimics
Flutter's proxy, where the app state changes only when an input event follows a
fresh real pointer press: typing by index (with no clear first, so only the type
path can supply the tap) and clearing by index. The others check that the
`flt-*`-tag and `data-semantics-role` halves of the detector each match and a plain
node does not, that an off-screen Flutter node is an error, and, as a control that
passes before and after, that a plain `<input>` gets no pointer event.

Each test was checked by mutation: skipping the Flutter branch in
`type_into_backend_node_id`, dropping the viewport guard, disabling the tag half of
the detector, and disabling the role half each turn exactly the matching test red.
The pre-fix `state=old` failure of the typing and clearing tests was observed
before the change. All four gate commands
exit 0 on this host: **207 passed, 0 failed** (205 before the review fixes, which
added the detection and off-screen tests).

The decisive check is manual, because the fixture encodes the cause hypothesis.
On a locally built Flutter Web 3.44.8 login page (headless, debug binary from
this tree): typing an email by index made the status read `welcome
probe@example.test`, replacing it read `welcome second@example.test`, clearing
with `text: ""` read `welcome `, and a password typed by index did not appear in
`browser_get_state`. The `browser_type` tool result still echoes a typed password
in plaintext, as before.

## Not changed

One Flutter version, one browser, headless only; an engine that tags its proxies
differently is treated as an ordinary page. A Flutter field outside the viewport
is not scrolled to. The cause was left open between a needed tap and a
focus-then-insert timing race; it was resolved on 2026-10-16 as page focus
([issue](../issues/2026-10-15-flutter-index-typing-drops-text.md#cause)). Nothing is
installed or deployed.

[Issue](../issues/2026-10-15-flutter-index-typing-drops-text.md) ·
[Closing the review gaps](2026-10-15-close-review-gaps.md) ·
[Changelog index](README.md)
