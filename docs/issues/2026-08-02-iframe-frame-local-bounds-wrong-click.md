# DOMSnapshot bounds are frame-local: iframe clicks hit the wrong element

Date: 2026-08-02 · Scope: Rust port (`bu-cdp`) selector map, clicks, and paint-order filtering

## Problem

`DOMSnapshot.captureSnapshot` reports every element's bounds **relative to its
own document**, with no offset for where an iframe sits in the page. The Rust
port dispatched synthetic mouse clicks at those raw coordinates, so a button
inside an iframe offset 300px down the page was "clicked" at its frame-local
position — which landed on a **different element in the main document**,
silently.

The same coordinate-space confusion also broke paint-order occlusion culling:
an opaque top-document header at `(0,0,400x120)` numerically "contained" an
iframe button's frame-local `(20,20,100x30)` rect and culled it — in the repro
it culled **every** interactive element on the page (upstream fixed the Python
side in `211091828`, "Scope paint-order filtering by iframe document").

## Evidence

- Live test: page with a top button (`TOP`) and an iframe button (`INNER`) at
  a frame-local position that overlaps the top button's screen location.
  Clicking the iframe button's index set the title to `TOP`, not `INNER`.
- Occlusion repro: adding an opaque top-document header made
  `interactive_elements` come back `[]`.

## Resolution

1. **Clicks** resolve their point through `DOM.getContentQuads`, which Chromium
   returns *relative to the viewport* with all frame offsets, scrolls, and
   transforms applied. Second gotcha found by an existing test: right after a
   JS DOM mutation, `getContentQuads` returns **pre-mutation** geometry (a
   reordered button reported its old slot), and `DOM.getDocument` does *not*
   refresh it — the stale part is layout, not the node tree. The click path
   forces a synchronous reflow (`document.documentElement.offsetHeight`) first.
2. **Occlusion** is scoped per owning DOMSnapshot `frameId` — only rects from
   the same document share a coordinate space. Python keys on
   `(session_id, frame_id)`; the Rust port is single-session per page, so
   `frame_id` alone is the equivalent key.

Commit `6f56f5638`; regression tests in `rust/crates/bu-mcp/src/tests.rs`
(`clicking_a_button_inside_an_offset_iframe_hits_that_button`,
`top_document_overlay_does_not_occlude_an_iframe_button`).

## Residual ceiling

Iframe elements' reported `x`/`y` in `browser_get_state` are still frame-local
(only the *click path* resolves through quads). Offsetting snapshot bounds into
top-document space would fix reported coordinates and make a global occlusion
union correct again; scoping was the smaller fix matching upstream's contract.
