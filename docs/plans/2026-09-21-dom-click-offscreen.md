# Plan: DOM-click offscreen or tiny nodes

## Table of Contents

- [Goal](#goal)
- [Why](#why)
- [Checklist](#checklist)
- [Verification](#verification)
- [Navigation](#navigation)

## Goal

`browser_click(index)` must activate Flutter Web's hidden
`flt-semantics-placeholder` ("Enable accessibility") so the semantics tree
populates. After that, `browser_get_state` lists real controls and the agent
can click/type by index instead of screenshot-and-guess coordinates.

## Why

Index clicks always dispatch a compositor mouse event at the node's box
centre. The placeholder is `1×1` at `(-1, -1)`, so that event misses the node.
Playwright's `getByRole(...).click({ force: true })` works because it is a
DOM activation. Match that for tiny or off-viewport boxes; keep mouse clicks
for in-viewport widgets (including Flutter semantic overlays once enabled).

Assumptions: `HTMLElement.click()` on `flt-semantics-placeholder` is what
Flutter's engine listens for. If live H5 still does not enable semantics,
record that as a follow-up rather than adding more click fallbacks in this
slice.

## Checklist

- [x] Open this plan
- [x] Failing test: offscreen 1×1 `<button>` is in the selector map and a
      mouse-centre click does not fire its handler
- [x] `Rect::prefers_dom_click` unit tests
- [x] `click_backend_node_id` uses `HTMLElement.click()` when the box is tiny
      or off-viewport; in-viewport boxes still use mouse
- [x] Headless test passes
- [x] Changelog + skill note
- [x] Release build installed (MCP client must reconnect to pick it up)
- [ ] Live H5 after reconnect: click index 0, `get_state` lists Log in / Email / terms

## Verification

```bash
cd rust
cargo test -p bu-cdp --lib -- --test-threads=1
cargo clippy -p bu-cdp --all-targets -- -D warnings
cargo fmt --check
```

Live H5 (after MCP clients pick up the new binary): login page →
`browser_click` index 0 → `flt-semantics-placeholder` absent and labelled
controls present.

## Navigation

- [Plans index](README.md)
- [Changelog](../changelogs/2026-09-21-dom-click-offscreen.md)
- [Flutter coordinate typing](../changelogs/2026-09-21-flutter-coordinate-typing.md)
