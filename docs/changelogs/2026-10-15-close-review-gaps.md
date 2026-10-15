# Closing the review gaps (2026-10-15)

## Table of Contents

- [What changed](#what-changed)
- [Behaviour you can observe](#behaviour-you-can-observe)
- [Verification](#verification)
- [Found on the way](#found-on-the-way)
- [Not changed](#not-changed)

## What changed

[The review of the MBP-local WIP](../issues/2026-10-15-mbp-local-wip-review-findings.md)
left four gaps. Each was closed with the resolution recommended there, except the
reveal-toggled password, which can be narrowed but not closed.

| Gap | Resolution |
| --- | --- |
| Live Flutter check open | Passed on a locally built Flutter Web 3.44.8 login page; recorded in the [DOM-click changelog](2026-09-21-dom-click-offscreen.md) and the [coordinate-typing changelog](2026-09-21-flutter-coordinate-typing.md) |
| Skill-sync CI job red | `scripts/sync_browser_harness_skill.py` re-run; `skills/browser-use/SKILL.md` and `browser_use/skills/browser-use/SKILL.md` regenerated from upstream `browser-harness` (identical to each other); `--check` exits 0 |
| Reveal-toggled password reported its live value | `bu-cdp/src/dom.rs` treats `placeholder`, `aria-label`, `name` and `id` containing `password`, `passwd`, `pwd`, `passcode`, `cvv` or `cvc` as sensitive, and `BrowserSession` keeps a per-tab set of backend node ids that ever looked sensitive (cleared on `navigate()`), shared across tab-switch handles |
| 10-minute stall on `forbidden_first_page_and_mutations_fail_closed` | `bu-cdp/src/lib.rs` `state()` reads the title with a raw `Runtime.evaluate` and no `contextId`; the test has a 120 s wall-clock bound |

## Behaviour you can observe

- A password field that a "show password" toggle flips to `type="text"` stays
  redacted in `browser_get_state` for the rest of the page. Navigating forgets it.
- `browser_get_state` no longer fails with `Cannot find context with specified
  id` while a script-initiated navigation commits.
- The two generated `SKILL.md` copies now carry upstream's current text, so a
  diff against the previous commit is large and none of it was written here.

## Verification

Run from `rust/` with the ambient `BROWSER_USE_*` variables unset:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features live-chrome -- -D warnings
cargo test --features live-chrome -- --test-threads=1
```

All four exit 0 on this host: **202 passed, 0 failed** (200 before, plus two live
tests). The new tests were checked by mutation. Against the pre-fix code
`state_survives_script_navigation_under_cpu_load` fails with `Cannot find context
with specified id`, and `a_password_field_stays_redacted_after_a_reveal_toggle_flips_its_type`
fails because the revealed value is reported. The policy test went from failing
in 4 of 8 runs to passing in 10 of 10 under ten CPU hogs.

Live Flutter probe (manual; needs the Flutter SDK): `flutter build web --release`
on a login page with an email field, an obscured password field, a `Log in`
button and a status text; serve it from loopback; drive `browser-use-rs --mcp`.
Results are in the two changelogs linked above.

## Found on the way

- `browser_type` by index into a Flutter field reports success and the app never
  receives the text; coordinates work. Open, recommendation only:
  [issue](../issues/2026-10-15-flutter-index-typing-drops-text.md).
- `masked_typed_text` masks by content shape (an email address), not by field
  type, so a password typed into a password field is echoed in the tool result.
  Unchanged; the agent already holds that string.
- About fifteen other `page.evaluate` call sites in `bu-cdp` share the stale
  execution-context race. Only `state()` is fixed;
  [learning record](../learning/2026-10-15-pinned-execution-context-races-navigation.md).
- The MCP server flag is `--mcp`. A bare `mcp` is not rejected: the binary takes
  it as an agent task and runs the LLM-backed agent against the configured
  provider. Unchanged; worth knowing before scripting the binary.

## Not changed

Nothing was deployed or installed on any host, the commit is local and not
pushed, and MBP-local's tree is untouched. The remaining deferred nits (10, 11
and the coordinate-mode remainder of 13) are still deferred. A residual
reveal-toggle gap remains for a field never seen as a password and with none of
the tells.

[Review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md) ·
[Changelog index](README.md)
