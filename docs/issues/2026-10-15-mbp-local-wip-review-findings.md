# Review findings on the MBP-local WIP (landed in `3c6a553e7`)

Date: 2026-10-15 · Scope: the three-slice Rust/docs WIP pulled from MBP-local
and committed locally (not pushed) · Status: **open, nothing fixed**

## Table of Contents

- [How it was reviewed](#how-it-was-reviewed)
- [Blocker](#blocker)
- [Should-fix](#should-fix)
- [Not verified](#not-verified)
- [Fix prompt](#fix-prompt)

## How it was reviewed

Five read-only reviewers (DOM-click, input tools, form-state security, test
integrity, docs vs code) over the working-tree diff, then two skeptics per
blocker/should-fix finding. **No reviewer ran cargo**, so no finding below was
reproduced by executing it, and any Chromium behaviour cited comes from the
reviewer's knowledge, not a run.

17 raw findings were confirmed and 0 refuted (consolidated below into 1 blocker and 12 should-fix rows, where overlapping findings share a row). Zero refutations is weak evidence on
its own (skeptics were shown the claim), so the column below says which
findings I re-read in the code myself. Gates on this host were green
(fmt, clippy `-D warnings` default and `live-chrome`, 174 tests pass), so none
of these is something the existing suite catches.

## Blocker

| # | Where | Problem | Checked |
| --- | --- | --- | --- |
| B1 | `bu-cdp/src/dom.rs` `is_sensitive_input` | Autocomplete redaction is `starts_with` on the whole attribute. `billing cc-number`, `section-pay cc-csc`, `shipping one-time-code` are valid token lists and bypass it, so the live `value` of a card or OTP field reaches `browser_get_state`. The lock test covers 3 of the filter's arms and no token list. | Read by me |

## Should-fix

| # | Where | Problem | Checked |
| --- | --- | --- | --- |
| S1 | `dom.rs` | Redaction keys only on `type=password`, `cc-*`, `one-time-code`. `autocomplete=current-password`/`new-password`, and a password field whose `type` was flipped to `text` by a reveal toggle, leak. | Reviewers |
| S2 | `dom.rs` | Live `value` has no length bound, unlike the 80-char `text`. | Reviewers |
| S3 | `bu-cdp/src/geometry.rs` `prefers_dom_click` | Tests the box's left/top edge, not the click point. A tall element at `y=-30` with its centre on screen loses the trusted mouse path and gets a synthetic `click()` (`isTrusted=false`, no pointerdown), which silently no-ops on Radix/react-select-style handlers while returning success. Right/bottom edges are never tested, contradicting the changelog's "in-viewport" row. The in-viewport live test passes under an always-DOM-click implementation, so it cannot catch this. | Reviewers |
| S4 | `bu-cdp/src/lib.rs` DOM-click | `this.click()` is undefined on SVG/MathML elements; the throw surfaces as a bare "Uncaught". | Reviewers |
| S5 | `lib.rs` `press_key` | Main key is dispatched with empty `commands`, so `Meta+a` / `Control+v` are silent no-ops that report success. This contradicts the changelog and the doc comment on `run_editing_command`, which says exactly that. | Read by me |
| S6 | `lib.rs` `press_key` | No `char` event, so `Enter` neither submits a form nor inserts a newline; `key_code` maps only 10 named keys, so Delete/Home/End/PageUp/PageDown/Space/Insert/F-keys get no virtual key code and do nothing. | Reviewers |
| S7 | `lib.rs` `paste_at_coordinates` | Order is click → clear → write clipboard → paste. On an insecure context `write_clipboard` fails *after* the field was wiped. The test asserts the error and never reads the field. | Read by me |
| S8 | `bu-actor/src/lib.rs` | The fail-closed policy test was not extended to the three new mutating commands, so policy gating of coordinate type, paste and press_key is unproven. | Reviewers |
| S9 | `skills/browser-use/SKILL.md` | Hand edit to a generated file. `scripts/sync_browser_harness_skill.py` regenerates both copies from upstream and its `--check` (CI job "browser-use skill sync") fails on any divergence. The copies were identical at `HEAD~1` and now differ. Expect red CI on push and the note lost on the next sync; move it to `docs/usage/`. | Script read, not run |
| S10 | `docs/changelogs/2026-09-21-upstream-sync-live-form-state.md` | "Binary rebuilt and installed" and "169 tests" are not reproduced here (this host: 174). MBP-local's installed binary is dated Sep 22, before edits through Sep 25, and its hash differs from its own build tree. The same file repeats the "redaction lock was validated by reverting it" paragraph. | Hashes read by me |
| S11 | `docs/practices/deploy-browser-use-rs.md` step 2, `docs/index.md`, HANDOFF | Probe still asserts 19 tools; `bu-mcp/src/tests.rs:31` asserts 21. Earlier statements about `e35b0a882` were true then; the probe instruction must change before the next deploy or it will fail a correct binary. | Read by me |
| S12 | `docs/plans/README.md` | Only the DOM-click slice has a board row and plan. Flutter coordinate typing and the upstream port have neither, and neither is in HANDOFF. | Reviewers |

## Not verified

Lower-ranked findings the review cap left unchecked: `write_clipboard` grants
the clipboard permission globally and never reverts it, overwrites the user's
clipboard and leaves the pasted text (possibly a credential) in it, and
force-raises the tab; `browser_type` does not enforce index XOR coordinates;
modifier aliases are sent verbatim as the `KeyboardEvent` key; the changelog's
"byte-identical to Python" claim is now false for `browser_type`. 18 nits were
not triaged. The claim that the redaction lock test fails with the filter
disabled is the changelog's, not reproduced here.

## Fix prompt

Fix B1 first with a failing test (token-list `autocomplete`), tokenising the
attribute on whitespace and matching any token, and add `current-password` and
`new-password`. Then S7 (check clipboard availability before clearing) and S5/S6
with a live-chrome test that asserts the field's resulting value. Move the S9
note out of the generated skill file. Re-run `cargo fmt --check`, clippy
`-D warnings` in both feature configurations, and the live-chrome suite. Do not
push or deploy before B1 is closed.

[Issues index](README.md) · [Landing commit context](../HANDOFF.md)
