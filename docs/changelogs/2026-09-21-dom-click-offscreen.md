# DOM-click offscreen and tiny nodes (2026-09-21)

## Table of Contents

- [Summary](#summary)
- [Behaviour](#behaviour)
- [Verification](#verification)

## Summary

`browser_click(index)` used a compositor mouse event at the node's box
centre. Flutter Web's `flt-semantics-placeholder` ("Enable accessibility")
is 1×1 CSS px at `(-1, -1)`, so that click never reached the node and the
semantics tree stayed empty. Tiny or off-viewport boxes now call
`HTMLElement.click()` (with `focus()` and CDP `userGesture`) on the node
itself. In-viewport widgets still get a mouse event at the centre, which
canvas overlays need.

## Behaviour

| Box | Action |
| --- | --- |
| Width and height both &lt; 2 CSS px | DOM `click()` |
| Left or top edge &lt; 0 | DOM `click()` |
| In-viewport, large enough | Mouse press/release at centre |
| No content quad | DOM `click()` |

## Verification

```bash
cd rust
cargo test -p bu-cdp --lib -- --test-threads=1
cargo test -p bu-cdp --features live-chrome click_element -- --test-threads=1
cargo clippy -p bu-cdp --all-targets -- -D warnings
```

The offscreen 1×1 button fixture fails under mouse-centre clicks (title stays
`Offscreen` or becomes `cover`) and passes once DOM-activated (title
`clicked`). An in-viewport `Log in` button still clicks.

Live mu H5 is checked after installing `browser-use-rs` and reconnecting the
MCP client (the login page's `Enable accessibility` placeholder). A cargo
test against that URL is not used: `navigate`'s 8s `stopLoading` plus
headless CanvasKit often never injects the placeholder.

[Plan](../plans/2026-09-21-dom-click-offscreen.md) · [Changelog index](README.md)
