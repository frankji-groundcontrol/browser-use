# Flutter coordinate typing (2026-09-21)

## Table of Contents

- [Summary](#summary)
- [New MCP tools](#new-mcp-tools)
- [browser_type change](#browser_type-change)
- [What the tests caught](#what-the-tests-caught)
- [Known limitation](#known-limitation)
- [Verification](#verification)

## Summary

Adds coordinate-based text entry for Flutter/canvas H5 apps, where
`browser_get_state` exposes no input indices because the page renders into a
single `<canvas>`. Coordinate clicking already existed; typing did not, so there
was no way to fill a form.

## New MCP tools

| Tool | Purpose |
| --- | --- |
| `browser_paste_at_coordinates` | Click (x,y) → clear → clipboard write → paste |
| `browser_press_key` | `Enter`, `Tab`, `Meta+a`, `Control+v`, … |

Both are unannotated (mutating), matching upstream's convention for tools that
change page state.

## browser_type change

`browser_type` now accepts **either** `index` **or** `coordinate_x` +
`coordinate_y`. Coordinate mode clicks the field, clears it (`clear`, default
true), then inserts the text. `text: ""` clears without typing, the same
contract index mode already had.

## What the tests caught

Writing the live-chrome tests first turned up three defects that a
build-and-eyeball pass had missed:

- **A synthetic accelerator does not run an editing shortcut.** Chromium
  resolves shortcuts in the browser process from a real OS key event, so a bare
  `Meta+A` over CDP left the selection untouched. "Replace the field" therefore
  deleted one character and appended, producing `old valuAlex`. Fixed by passing
  the editing command explicitly via `Input.dispatchKeyEvent`'s `commands`
  field, which also removes any dependence on the browser's host OS.
- **A modifier's `code` is side-specific.** Sending `Meta` rather than
  `MetaLeft` made Chromium drop it, and the page saw an empty `event.code`.
  Modifiers now resolve through the same `key_code` table as the main key.
- **`writeText` needs more than the read permission.** It is gated on
  `clipboardSanitizedWrite` and refuses to run in an unfocused document, so a
  background automation tab has to be brought to the front first.

An unrecognised modifier is now an error instead of a zero bit; silently
degrading `Hyper+v` to a plain `v` reported success for a no-op. `Ctrl`, `Cmd`,
and `Option` are accepted as aliases.

## Known limitation

`browser_paste_at_coordinates` needs `navigator.clipboard`, which only exists in
a secure context (https, or http on localhost). On a plain `http://` page it
fails with a message pointing at `browser_type` coordinate mode instead.

## Live check (2026-10-15)

On a locally built Flutter Web 3.44.8 login page (headless Chromium, debug binary
from this tree, not installed), `browser_type` with `coordinate_x`/`coordinate_y`
into the Email field followed by `Log in` made the app's status label read
`welcome <typed email>`. The password field was typed the same way and its text
never appears in `browser_get_state`. `browser_paste_at_coordinates` and
`browser_press_key` were not exercised on that page. Index-mode typing on the
same page silently drops the text:
[issue](../issues/2026-10-15-flutter-index-typing-drops-text.md).

## Verification

```bash
cd rust
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --features live-chrome -- --test-threads=1
cargo build -p bu-core --release
install -m 755 target/release/browser-use-rs ~/.local/bin/browser-use-rs
codesign -f -s - ~/.local/bin/browser-use-rs
```

Restart MCP clients after installing so the tool schemas refresh.

[Changelog index](README.md)
