# DOM-click offscreen and tiny nodes (2026-09-21)

## Table of Contents

- [Summary](#summary)
- [Behaviour](#behaviour)
- [Verification](#verification)

## Summary

`browser_click(index)` used a compositor mouse event at the node's box
centre. Flutter Web's `flt-semantics-placeholder` ("Enable accessibility")
is 1×1 CSS px at `(-1, -1)`, so that click never reached the node and the
semantics tree stayed empty. Tiny nodes, and nodes whose box
centre is outside the viewport, now get `click()` (after `focus()`, with CDP
`userGesture`) on the node itself; SVG and MathML elements have no `click()`, so
a `click` event is dispatched instead. Widgets whose centre is in the viewport
still get a trusted mouse event at the centre (`isTrusted`, with `pointerdown`),
which canvas overlays and pointer-driven widgets need.

## Behaviour

| Box | Action |
| --- | --- |
| Width and height both &lt; 2 CSS px | DOM `click()` |
| Box centre outside the viewport (x or y &lt; 0, or at or past its width or height) | DOM `click()` |
| Centre inside the viewport and large enough, including a tall element only partly scrolled in | Mouse press/release at centre |
| No content quad (hidden or detached) | Refused unless the node is still in the live selector map, then DOM `click()` |

The first version tested only the box's left and top edge, which sent a tall,
partly scrolled element down the untrusted DOM path and dropped its
`pointerdown`; the 2026-10-15 fix pass moved the test to the click point.

## Verification

```bash
cd rust
cargo test -p bu-cdp --lib -- --test-threads=1
cargo test -p bu-cdp --features live-chrome click_ -- --test-threads=1
cargo clippy -p bu-cdp --all-targets -- -D warnings
```

The offscreen 1×1 button fixture fails under mouse-centre clicks (title stays
`Offscreen` or becomes `cover`) and passes once DOM-activated (title
`clicked`). In-viewport buttons are asserted to receive a trusted
`pointerdown` and `click`.

Live check (2026-10-15, **closed**): a locally built Flutter Web 3.44.8 login
page (CanvasKit, headless Chromium) driven over `browser-use-rs --mcp` by a debug
build from this tree, not installed anywhere. After a 3 s wait `browser_get_state`
listed two elements, `Enable accessibility` at index 0 and `flutter-view`.
`browser_click(0)` removed the placeholder and added 10 `flt-semantics` nodes; the
next state listed `Email`, `Password` and `Log in` (four elements). Clicking
`Log in` by index changed the app's status label from `signed out` to `welcome `.
Headless CanvasKit did inject the placeholder for this build, so the earlier worry
that it never would did not hold here; another Flutter version or a hosted app
may differ. No cargo test drives this page, because it needs the Flutter SDK; the
check is a manual probe. The same page showed that `browser_type` by index
silently dropped text; [fixed afterwards](2026-10-15-flutter-index-typing.md).

[Plan](../plans/2026-09-21-dom-click-offscreen.md) · [Changelog index](README.md)
