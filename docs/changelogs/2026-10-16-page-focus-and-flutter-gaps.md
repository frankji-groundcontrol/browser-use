# Page focus, Flutter scrolling, and context-free script evaluation (2026-10-16)

## Table of Contents

- [What changed](#what-changed)
- [Behaviour you can observe](#behaviour-you-can-observe)
- [Verification](#verification)
- [Not changed](#not-changed)

## What changed

**The Flutter index-typing cause is now measured, and the workaround is gone.**
The [2026-10-15 fix](2026-10-15-flutter-index-typing.md) tapped Flutter fields
with a real click before typing and left the cause open. The cause is page
focus. The MCP actor adopts Chromium's initial tab, and that tab reports
`document.hasFocus() === false`; a tab opened with `new_page()` reports `true`.
With the fix disabled the initial tab was unfocused after `about:blank`, a `data:`
page and a loopback `http://` page alike. `DOM.focus` alone left it unfocused, and
it was focused by the end of `type_into_backend_node_id`, so page focus flipped
inside the insert step. Flutter Web dropped the text that arrived with that
transition. A real mouse press focuses the page, which is why tapping first
appeared to fix it.

`bu-cdp/src/lib.rs` now sends `Emulation.setFocusEmulationEnabled(true)` on the
pages it drives (`new_page`, the primary page, and a tab switched to), so the page
is focused before any typing. It does so only in a browser nobody is watching: one
the server launched, or an attached one whose user agent says `HeadlessChrome`
(`Browser.getVersion` reports plain `Chrome/…` for headless, so the product string
cannot tell). An attach to your own headed Chrome gets none, and `close_tab` no
longer sends it. `Page.bringToFront` also works but would raise a real window in
headed mode. A failure to enable it logs a warning and carries on.
The Flutter detector, the tap, the keyboard clear and the off-screen error added
on 2026-10-15 are deleted: index typing is the plain focus-and-insert path again
on every page.

**`browser_scroll` scrolls whatever is under the viewport centre.** Flutter Web
draws its own scrollables inside a window that never scrolls, so
`window.scrollBy` did nothing. `scroll()` now asks whether the window scrolls at
all: taller than the viewport, and no `overflow: hidden`/`clip` on the viewport
(from `<html>`, or from `<body>` when `<html>` leaves it visible). If so it calls
`window.scrollBy`, which is an inert no-op at either end. If not, it dispatches a
mouse wheel of ±500 at the viewport centre. The script uses `window.`-prefixed
names so a page's top-level `let scrollBy` cannot shadow them.

**Script evaluation no longer pins an execution context.** A new `eval` helper
sends `Runtime.evaluate` with no `contextId`, `awaitPromise` and `returnByValue`.
It replaces `Page::evaluate` at the fifteen remaining call sites, plus `state()`
and `content()`. A thrown script surfaces as `JavaScript exception: <description>`.
See [the learning record](../learning/2026-10-15-pinned-execution-context-races-navigation.md).

**Review nits 10, 11 and 13 are answered** in
[the review-findings record](../issues/2026-10-15-mbp-local-wip-review-findings.md#nits):
Chrome-free tests were added where the logic is Chrome-free.

## Behaviour you can observe

- `browser_type` by index into a Flutter field reaches the app on the first fill,
  and replacing and clearing (`text: ""`) work, without any tap.
- A button or checkbox passed to `browser_type` is no longer tapped; the call
  focuses it and inserts nothing visible.
- A page we drive in a launched or headless browser reports itself focused. An
  attach to your own headed Chrome (`BROWSER_USE_CDP_URL`) is left alone: the
  server does not change what your tabs believe about focus, and the state it
  would leave behind (reported until the connection drops) is not worth the risk.
- `browser_scroll` on a window that cannot scroll at all (Flutter, an open modal,
  an app shell with an inner scroller) sends a wheel event to the viewport centre.
  A page that does scroll is never wheeled, including at its end, so a map or
  carousel under the pointer is not nudged.
- On Flutter, the first 500 px `browser_scroll` may reveal nothing new; the lazily
  built nodes appeared after the second call.
- A thrown script reports `JavaScript exception: …` instead of the underlying
  chromiumoxide error text.

## Verification

Run from `rust/` with the ambient `BROWSER_USE_*` variables unset:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features live-chrome -- -D warnings
cargo test --features live-chrome -- --test-threads=1
```

All four exit 0 on this host: **212 passed, 0 failed** (207 before this change). The five Flutter
proxy tests from 2026-10-15 were removed with the code they covered.

Each new test was checked by mutation and turns red for the named cause:

| Test | Mutation | Result |
| --- | --- | --- |
| `the_initial_tab_is_focused_before_typing_by_index` | focus emulation set to `false` | fails: `the page must be focused before any typing` |
| `switch_tab_focuses_a_tab_this_session_never_drove` (a second session attaches to the launched Chromium) | `switch_tab` uses `wrap` instead of `driven` | fails: `the page must be focused before any typing` |
| same | the attach path never turns emulation on | fails the same way |
| `focus_emulation_is_for_browsers_nobody_is_watching` (no Chrome) | the predicate tests the product string instead of the user agent | fails (the first draft did, and the attach test caught it) |
| `scroll_moves_an_inner_container_when_the_window_cannot_scroll` | wheel fallback removed | fails: `the container never scrolled` |
| `scroll_goes_to_a_dialog_when_a_modal_locks_the_page` | overflow lock check removed | fails: `the container never scrolled` |
| `scroll_at_the_end_of_a_scrollable_page_sends_no_wheel` | wheel again at the end of a scrollable window; bare `scrollBy` | each fails |
| `state_survives_script_navigation_under_cpu_load` (now also polls `scroll_y` and `content`) | `scroll_y` back on `Page::evaluate` | fails: `Cannot find context with specified id` |
| `typing_rejects_a_single_coordinate_and_names_the_pair` (no Chrome) | the single-coordinate branch removed | fails: the error says `requires an unsigned integer index` |
| `typed_text_is_masked_in_the_tool_echo_by_shape` (no Chrome) | email length floor raised; credential floor raised | both fail |
| `icon_file_names_come_from_the_url_path_only` (no Chrome) | `data:` check made case-sensitive; `?` no longer a separator; `#` no longer a separator | each fails (the `?v=2#x` row alone hid the `#` case, so each separator now has its own row) |
| `typing_at_coordinates_masks_an_email_in_the_result` | coordinate echo bypasses the mask | fails |

`a_tab_switched_back_to_is_focused` was replaced: the session had already driven
that tab, so reverting `switch_tab` left it green. The replacement reaches the
initial tab from a session that never drove it. `new_page` has no such test: a new
tab is natively focused (measured), so its `driven` call is belt and braces that no
test can turn red.

Two earlier tests did not pass this check and were rewritten. As first written
they asserted that a `focus` handler had run, and it had run by the time the
assertion read it, with or without the fix, so they stayed green with emulation
off. They now assert `document.hasFocus()` before any typing.

The decisive check is manual, on locally built Flutter Web 3.44.8 apps served from
loopback and driven over `browser-use-rs --mcp` (headless, debug build from this
tree):

- Login page, CanvasKit build. Typing the email by index first, with the old code,
  failed in three of five orderings (`email_only`, `email_then_pw`, `email_wait`)
  and worked in two (`email_twice`, `pw_then_email`); the two that worked do a
  second fill after the focus transition. With the fix all five read
  `welcome e1@x.test`. Replacing read `welcome second@example.test`, clearing read
  `welcome `, and a password typed by index did not appear in `browser_get_state`.
- The same login page as a WasmGC build (`main.dart.wasm` loaded): first fill,
  replace, clear and the password check all pass.
- Scroll page. After two `browser_scroll` calls `Far email`, `Agree` and `Submit`
  appear. Typing into `Submit` and `Agree` left `submits:0` and `agree:false`.
  Typing into `Far email`, clicking `Agree` and clicking `Submit` read
  `email=far@x.test`, `agree:true`, `submits:1`.

## Not changed

- **Why Flutter drops the text is not traced into the engine.** The claim rests on
  the matrix: a new tab works; the initial tab fails; the initial tab with
  `bringToFront` works; the initial tab with focus emulation works. The Flutter
  framework's focus and lifecycle handling was not read for this.
- **A headed attach gets no focus emulation.** If you attach to your own Chrome
  and the tab the agent types into is not focused (window in the background), a
  Flutter field may still drop its first fill. Untested; the fix there is to
  bring the tab forward, which `browser_switch_tab` already does.
- **Headed mode and real devices are untested.** Running headed would raise a
  window on the desktop of whoever runs the check. Flutter 3.44.8 is the only
  version tried, as CanvasKit and as a WasmGC build; the renderer name behind the
  WasmGC build was not confirmed independently.
- **A never-seen password field flipped to `text`, with no password-like name, id,
  placeholder, label or autocomplete, is still reported with its live value.** It
  cannot be recognised from a snapshot, and closing it would mean redacting every
  text field. Accepted; see
  [the review record](../issues/2026-10-15-mbp-local-wip-review-findings.md#accepted-and-deferred).
- **Regression coverage of the context-free evaluation is three sites** (`state`,
  `scroll_y`, `content`). The other sites share the helper but only `scroll_y` was
  mutated back to `Page::evaluate`.
- **Nothing is deployed.** The installed binary on every host predates this work,
  and the MBP-local tree was not touched.

[Flutter index typing, first fix](2026-10-15-flutter-index-typing.md) ·
[Issue](../issues/2026-10-15-flutter-index-typing-drops-text.md) ·
[Changelog index](README.md)
