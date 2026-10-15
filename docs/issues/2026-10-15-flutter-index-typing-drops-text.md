# `browser_type` by index reports success but Flutter Web never receives the text

Date: 2026-10-15 · Status: **open, recommendation only** (no code changed) ·
Found by: the live Flutter check in
[the DOM-click plan](../plans/2026-09-21-dom-click-offscreen.md)

## Table of Contents

- [Observed](#observed)
- [Reproduction](#reproduction)
- [Likely cause](#likely-cause)
- [Recommendation](#recommendation)
- [Not proven](#not-proven)

## Observed

On a real Flutter Web 3.44.8 login page (CanvasKit, headless Chromium, locally
built), after `browser_click` on the `Enable accessibility` placeholder:

| Step | Result |
| --- | --- |
| `browser_type` with `index` on the Email field | Returns `Typed <email> into element 1`; the page ignores the text |
| Press `Log in` afterwards | Status label reads `welcome ` (empty), identical to a run that never typed |
| `browser_type` with `coordinate_x`/`coordinate_y` on the same field | Same success message; status label reads `welcome probe@example.test` |

So the text reaches the app only in coordinate mode. Index mode is a silent
false success, which is worse than an error: an agent will carry on believing
the form is filled.

## Reproduction

A 20-line Flutter app (a `TextField`, an obscured `TextField`, a button, and a
`Text` showing `welcome ${email.text}`), `flutter build web --release`, served
from loopback, driven over `browser-use-rs --mcp`: navigate, wait 3 s, click the
placeholder, `browser_type` by index into `Email`, click `Log in`, then read the
status node with `browser_get_html` and a `#flt-semantic-node-N` selector. The
typed run and a no-typing control produce the same label.

## Likely cause

`type_into_backend_node_id` focuses the node with `DOM.focus` and then sends
`Input.insertText`. `type_at_coordinates` first sends a real mouse click. Flutter
turns a pointer tap into a framework-level focus of the `TextField`, which then
owns a text-editing host that accepts `insertText`. A programmatic DOM focus on
the semantics proxy `<input>` is not that tap. After the index-mode type the
proxy's `autocorrect` attribute had flipped from `off` to `on`, so the engine
reacted to the focus, but the text still did not arrive.

## Recommendation

When the target is a Flutter semantics node (an ancestor `flutter-view` or an
`flt-semantics` host), route `browser_type(index)` through the trusted-click path:
click the element's box centre, then clear and insert exactly as
`type_at_coordinates` does. Add a live test that builds on the existing Flutter
fixture idea: a page that mimics the proxy input and records whether the text
arrived by a pointer-driven handler. Do not add a post-hoc "did the value
change" check as the only fix, because Flutter's proxy `<input>` does not
reliably mirror the app's text.

## Not proven

- The cause above is an explanation consistent with the evidence, not a
  verified one; no Flutter engine source was read and no breakpoint was set.
- One Flutter version, one browser, headless only. A real device build or a
  different renderer may differ.
- The password field was typed through coordinates and its text never appeared
  in `browser_get_state`. That is consistent with redaction by `type="password"`,
  but it was not distinguished from Flutter simply not mirroring the value.

Related: [Flutter coordinate typing](../changelogs/2026-09-21-flutter-coordinate-typing.md) ·
[Issues index](README.md)
