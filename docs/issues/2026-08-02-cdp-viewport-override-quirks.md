# CDP viewport-override quirks: 0x0 doesn't clear, mobile falls back to 980px

Date: 2026-08-02 · Scope: `browser_set_viewport` (Rust MCP tool), `Emulation` domain

## Problem

Two behaviors of `Emulation.setDeviceMetricsOverride` contradict a plain
reading of the protocol docs; both were caught by the live test rather than
reasoning.

## Quirk 1 — a 0x0 set does NOT restore the window size

The protocol docs say a width/height of 0 "disables the override" for that
dimension. In practice a `setDeviceMetricsOverride` with `width=0, height=0`
left the **previous override fully in force** — the page stayed at the emulated
width, and `mobile` stayed applied. Clearing must go through
`Emulation.clearDeviceMetricsOverride`.

## Quirk 2 — `mobile: true` on a non-responsive page lays out at 980px

Mobile emulation is real device emulation: a page **without**
`<meta name=viewport>` gets Chrome's 980px fallback layout viewport — exactly
like a real phone — regardless of the width passed. Only a
`width=device-width` page lays out at the requested width. A test asserting
`window.innerWidth == 377` on a plain page fails with `980`; that's correct
browser behavior, not a bug in the tool.

## Resolution

`BrowserPage::set_viewport` routes `0x0` to `clearDeviceMetricsOverride`
(commit `39b1620de`), and the live test
(`set_viewport_changes_css_width_and_clears` in `bu-mcp/src/tests.rs`) pins
both mobile paths — 980 fallback and `device-width` at the set width — so the
surprise is documented instead of rediscovered.

Related test-authoring gotcha from the same session: in a `data:` URL,
`background:#fff` truncates the document at the `#` (URL fragment). Use
`background:white` or percent-encode.
