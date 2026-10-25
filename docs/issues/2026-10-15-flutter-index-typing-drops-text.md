# `browser_type` by index reports success but Flutter Web never receives the text

Date: 2026-10-15 · Status: **fixed locally, not deployed; cause measured and fix
replaced 2026-10-16** · Found by: the live Flutter check in
[the DOM-click plan](../plans/2026-09-21-dom-click-offscreen.md) · Change records:
[first fix](../changelogs/2026-10-15-flutter-index-typing.md) (tap first, superseded) ·
[page focus](../changelogs/2026-10-16-page-focus-and-flutter-gaps.md) (current)

## Table of Contents

- [Observed](#observed)
- [Reproduction](#reproduction)
- [Cause](#cause)
- [Resolution](#resolution)
- [Not proven](#not-proven)

## Observed

On a real Flutter Web 3.44.8 login page (CanvasKit, headless Chromium, locally
built), after `browser_click` on the `Enable accessibility` placeholder:

| Step | Result before the fix |
| --- | --- |
| `browser_type` with `index` on the Email field | Returns `Typed <email> into element 1`; the page ignores the text |
| Press `Log in` afterwards | Status label reads `welcome ` (empty), identical to a run that never typed |
| `browser_type` with `coordinate_x`/`coordinate_y` on the same field | Same success message; status label reads `welcome probe@example.test` |

So the text reached the app only in coordinate mode. Index mode was a silent
false success, which is worse than an error: an agent will carry on believing
the form is filled. Clearing a field by index (`text: ""`) had the same flaw.

## Reproduction

A 20-line Flutter app (a `TextField`, an obscured `TextField`, a button, and a
`Text` showing `welcome ${email.text}`), `flutter build web --release`, served
from loopback, driven over `browser-use-rs --mcp`: navigate, wait for the
`Enable accessibility` placeholder, click it, `browser_type` by index into
`Email`, click `Log in`, then read the status node with `browser_get_html` and a
`#flt-semantic-node-N` selector. The typed run and a no-typing control produced
the same label.

## Cause

The MCP actor adopts Chromium's initial tab, and that tab reports
`document.hasFocus() === false`. A tab opened with `new_page()` reports `true`.
Measured with the fix off: the initial tab was unfocused on `about:blank`, on a
`data:` page and on a loopback `http://` page. `DOM.focus` alone left it
unfocused, and it was focused by the end of `type_into_backend_node_id`, so page
focus flipped inside the insert step. Flutter Web dropped the text that arrived
with that transition.

The orderings on the real login page, with the old focus-and-insert code, fit
that: typing the email first, with or without a password after it or a wait before
it, failed (`email_only`, `email_then_pw`, `email_wait`); typing the email twice, or
the password before the email, worked (`email_twice`, `pw_then_email`), because the
second fill comes after the transition. Through `bu-cdp` directly: new tab works,
initial tab fails, initial tab plus `Page.bringToFront` works, initial tab plus
`Emulation.setFocusEmulationEnabled(true)` works. A real mouse press focuses the
page, which is why the first fix, tapping the field first, appeared to work.

Not traced: why the Flutter framework drops text that arrives with the transition.
The earlier explanation (a needed pointer tap, or a focus-then-insert race inside
the engine) is withdrawn; the matrix does not need it.

## Resolution

`bu-cdp` sends `Emulation.setFocusEmulationEnabled(true)` on the pages it drives in
a browser nobody is watching (launched, or attached and headless), so the page is
focused before any typing. This replaced the 2026-10-15 fix
(a Flutter detector that tapped the node, cleared with the keyboard and refused
off-screen nodes), which is deleted. Index typing is the plain focus-and-insert
path on every page, so a button or checkbox given to `browser_type` is no longer
tapped.

Evidence, in [the change record](../changelogs/2026-10-16-page-focus-and-flutter-gaps.md#verification):

- Two live tests assert `document.hasFocus()` before any typing: on the initial
  tab, and on a tab reached through `switch_tab` by a second session that never
  drove it. Both fail with emulation off, and the second fails when `switch_tab`
  stops driving. Two earlier tests asserted that a `focus` handler had run; they
  stayed green with emulation off and were rewritten.
- On the real Flutter 3.44.8 login page, CanvasKit and WasmGC builds: all five
  orderings read `welcome e1@x.test`, replace and clear work, and a typed password
  stays out of `browser_get_state`.

## Not proven

- One Flutter version (3.44.8), headless only, as CanvasKit and as a WasmGC build.
  A real device build, other versions, and headed mode were not tried.
- Why the framework drops the text (see Cause).
- Off-viewport Flutter nodes: not scrolled to by `browser_type`. A lazy list only
  builds nodes near the viewport, so the nodes appear after `browser_scroll`, which
  now sends a wheel event when the window cannot scroll.
- A headed attach to your own Chrome gets no emulation (by design), so an
  unfocused background tab there may still drop the first fill. Untested.
- Nothing is installed or deployed; the real-Flutter runs used a debug build from
  this tree.

Related: [Flutter coordinate typing](../changelogs/2026-09-21-flutter-coordinate-typing.md) ·
[Issues index](README.md)
