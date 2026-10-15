# `browser_type` by index reports success but Flutter Web never receives the text

Date: 2026-10-15 · Status: **fixed locally, not deployed** · Found by: the live
Flutter check in [the DOM-click plan](../plans/2026-09-21-dom-click-offscreen.md) ·
Change record: [Flutter index typing](../changelogs/2026-10-15-flutter-index-typing.md)

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

`type_into_backend_node_id` focused the node with `DOM.focus` and then sent
`Input.insertText` immediately; `clear_backend_node_id` reset the proxy
`<input>`'s value with the native setter. `type_at_coordinates` first sends a real
mouse click and waits 500 ms. After the index-mode type the proxy's `autocorrect`
attribute had flipped from `off` to `on`, so the engine reacted to the focus, but
the text still did not arrive.

The mechanism is **not established**. Two explanations fit. One: the engine needs
a real pointer tap, and a programmatic focus is not one. Two: a timing race. The
engine's text-field semantics (`text_field.dart` in the local Flutter SDK, read
on 2026-10-15) answer a DOM `focus` event with a `SemanticsAction.focus` to the
framework, and activate the text-editing strategy only on the next semantics
update, so `insertText` sent straight after `DOM.focus` can land before the
field is active. The fix taps and then waits, which covers both; the two were not
told apart.

## Resolution

`bu-cdp` now recognises a Flutter semantics node with one `DOM.describeNode`: an
`flt-*` tag name, or a `data-semantics-role` attribute (the marker on the proxy
`<input>`/`<textarea>`). For such a node, `browser_type(index)` taps the node's
box centre with a real mouse click, then clears with the keyboard (select all and
Backspace) and inserts, which is the sequence the coordinate path already used.
Ordinary pages take the old path unchanged.

Evidence:

- Two live tests drive a page that mimics the proxy: the app state only changes
  when an input event follows a fresh real pointer press. Typing (with no clear
  first) and clearing by index fail against the old code and pass now. Three more
  cover the rest: a control that a plain `<input>` gets no pointer event, the
  `flt-*`-tag and `data-semantics-role` halves of the detector, and an error for
  an off-screen Flutter node.
- On the real Flutter 3.44.8 page after the fix: typing an email by index made the
  status read `welcome probe@example.test`, replacing it read `welcome
  second@example.test`, and clearing with `text: ""` read `welcome `. A password
  typed by index did not appear in `browser_get_state`.

## Not proven

- The cause (see above) is open between a needed tap and a timing race. The test
  page encodes the tap assumption, so the real-Flutter run is the decisive check.
  Whether a value reset after a tap would also have worked was not tried; the
  keyboard clear is the sequence already proven live in coordinate mode.
- One Flutter version, one browser, headless only. A real device build, another
  renderer, or an engine that tags its proxies differently may differ; an
  unrecognised engine is treated as an ordinary page, which is the old behaviour.
  In the local engine source the `data-semantics-role` marker is set on both the
  single-line and multi-line proxies.
- A Flutter field outside the viewport (or under 2×2 px, like the accessibility
  placeholder) is not scrolled to: `browser_type` returns an error instead, because
  a missed tap still reports success and the keyboard would then edit whichever
  field was focused before. Marked with a `ponytail:` comment in the code.
- Typing taps the field twice (once to clear, once to type), which is harmless for
  a text field. Any other `flt-*` node given to `browser_type`, such as a button
  or checkbox, is tapped twice as well; the old path only focused it.
- Nothing is installed or deployed; the real-Flutter run used a debug build from
  this tree.

Related: [Flutter coordinate typing](../changelogs/2026-09-21-flutter-coordinate-typing.md) ·
[Issues index](README.md)
