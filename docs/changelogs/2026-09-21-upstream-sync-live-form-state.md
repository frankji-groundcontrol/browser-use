# Upstream sync: live form state and icon labels ported (2026-09-21)

Practice: [upstream sync and port](../practices/upstream-sync-and-port.md)

## Table of Contents

- [Scope](#scope)
- [Ported](#ported)
- [Already covered, now locked](#already-covered-now-locked)
- [Refuted](#refuted)
- [Not assessed](#not-assessed)
- [Deliberate divergences](#deliberate-divergences)
- [Verification](#verification)

## Scope

Surveyed `upstream/main` (`d8110c5ff`) against merge base `28670f720`
(2026-08-26): **224 commits**, 45 files under `browser_use/`. No merge was
performed — this was a diff-and-port pass only, so the fork's Python tree is
still at the old base.

Upstream's MCP tool set is unchanged (16 tools, identical names), so the 504-line
churn in `mcp/server.py` is whitespace plus one behavioral commit.

## Ported

### Live form state in the selector map

From `5ffd3090a` and its security follow-up `3b12a946c`, which must land
together.

DOM attributes only carry the `value=` written in the HTML. When JavaScript,
autofill, or a framework fills a field, the value lives on the element property,
which `DOMSnapshot.captureSnapshot` reports in the sparse `inputValue` /
`textValue` / `inputChecked` arrays. `browser_get_state` could not distinguish a
filled field from an empty one; the same blind spot made QA on a Flutter H5 app
fall back to `evaluate("….value")` to check its own work.

`SelectorMapElement` gains `value` and `checked`, surfaced in
`browser_get_state`. Sensitive fields — `type` password/file/hidden, and
`autocomplete` starting `cc-` or `one-time-code` — are dropped in
`live_input_values` before anything is written to `EnhancedNode`, so no later
change to serialization can leak one.

The fork already solved the same class of problem for `<select>` via
`enrich_select_text`; this extends it to inputs.

### Image labels for icon-only controls

From `96117ef8f` and its three follow-ups, but **narrower than upstream**, because
most of it turned out to be covered already: the accessibility tree computes a
button's name from a child `<img alt=…>`, so `<button><img alt="Delete item"></button>`
was labelled correctly before this change. Only the no-`alt` case was blind.
`descendant_image_label` now falls back to the image's file name, skipping `data:`
URIs, which carry bytes rather than a name.

## Already covered, now locked

| Upstream | Rust status |
| --- | --- |
| `afbc714a6` — an unknown MCP tool must never return success | `call_browser_tool` already returned `METHOD_NOT_FOUND`. Upstream reports `is_error=true` instead; both are errors, and a protocol error is the closer reading of the MCP spec. Nothing asserted it, so a test was added. |
| `0a6e427c6` + `2b56dc8f5` — literal `+` in a key combo | No `send_keys` counterpart exists, but `split_key_combo` (added the same day, for `browser_press_key`) solves the same parsing problem and was untested for `+` and `Shift++`. Upstream's contract was ported as a test; the implementation already satisfied it. |
| `610c9614a` — honor `BROWSER_USE_HEADLESS` | `headless_from_env` already reads it with the same falsy set. Diverges only on an empty value, which upstream treats as headed and this fork as headless. |

## Refuted

Not portable, because the code that carries the bug was never ported:

- `17cbc07ec` normalizes a hidden-`<code>` style check — that LinkedIn
  hidden-JSON heuristic lives in `html_serializer.py`; `extract_clean_markdown`
  has no equivalent.
- `2f573908c` clears cookies through the Storage domain — no cookie-clearing
  surface exists here.
- `utils.py` URL extraction from task text — no counterpart.
- `filesystem/file_system.py` (233 lines) — no counterpart, same as the
  [2026-08-27 sync](2026-08-27-upstream-sync-nothing-to-port.md).
- LLM providers (new `orcarouter`, plus ollama/deepseek/openrouter edits) — no
  provider classes; the model comes from `BROWSER_USE_LLM_MODEL`.

## Not assessed

Four commits touch subsystems where this fork has its own implementation from the
hardening pass, and were **not** verified to the standard this practice asks for:
session handler restoration after an event-bus reset (`554ce8f99`), reconnect drop
handling (`2c3162069`), Chrome argument order (`df2701e52`), and early
browser-exit detection during launch (`acb3f0efb`).

## Deliberate divergences

- **`value` and `checked` are fields, not attributes.** Upstream writes the live
  value into `attributes['value']`. Here `text` is a *label*, and overwriting it
  with user-typed content would both weaken identification and push field
  contents into a label for inputs with no placeholder.
- **Each field is reported only where it means something.** `inputChecked` covers
  the whole document, so without a type test every text input would claim
  `checked: false`; and a checkbox's value is the constant `"on"`, which says
  nothing its checked state doesn't. Both were caught by test assertions after the
  first green run.

## Verification

```bash
cd rust
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --features live-chrome -- --test-threads=1
```

The redaction lock was validated by reverting it: with the `is_sensitive_input`
filter disabled, `get_state_never_exposes_sensitive_live_values` fails with the
password, one-time code, and card number all present in the state payload.
*(The original author's claim, not re-run as written. The 2026-10-15 fix pass
re-proved the lock against the pre-fix code instead; see
[the review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md).)*

All green on MBP-local: **169 tests, 0 failures** in 4m50s, with `fmt --check`
and `clippy -D warnings` clean. That figure is the original author's and was not
reproduced; the same tree on the pulling host ran 174 tests (the difference was
not investigated). The original
note that the binary was "rebuilt and installed" does **not** hold: MBP-local's
installed binary was dated before the last edits and its hash differed from its
own build tree. Nothing from this tree has been deployed.

### A note on the environment, not the code

Two earlier attempts at this run were worthless, and the reason is worth
recording because it looks exactly like a product defect:

- `live_tests::attaching_uses_an_existing_browser_and_never_closes_it` failed
  once, then passed on its own in 1.86s.
- `policy_tests::forbidden_first_page_and_mutations_fail_closed` then appeared to
  hang: no output for ten minutes, the test binary at 0% CPU, its Chromium
  processes all asleep. On its own it finished in 7.34s.

The cause was a resident antivirus file-scanner scanning every file a fresh
Chrome profile writes, on a machine already at load 57–134. Once that scanner was
stopped, the same bu-actor suite went from "hung past ten minutes" to **11.86s**.

Worth knowing for next time: these tests construct `BrowserActor` directly rather
than going through `ActorHandle::request`, so the 90s command timeout does not
apply and a starved browser launch has nothing to cut it short. A sample of the
process (0% CPU, parked in `block_on`) distinguishes starvation from a real
deadlock in seconds — much faster than waiting.

[Changelog index](README.md)
