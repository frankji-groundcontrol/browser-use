# Review findings on the MBP-local WIP (landed in `3c6a553e7`)

Date: 2026-10-15 · Scope: the three-slice Rust/docs WIP pulled from MBP-local
and committed locally · Status: **resolved on this host** (every finding fixed
with a red→green check, or accepted/deferred below with a reason); **not
deployed**, and MBP-local's own tree is untouched

## Table of Contents

- [How it was reviewed](#how-it-was-reviewed)
- [Resolution](#resolution)
- [Nits](#nits)
- [Accepted and deferred](#accepted-and-deferred)
- [What this does not prove](#what-this-does-not-prove)

## How it was reviewed

Five read-only reviewers (DOM-click, input tools, form-state security, test
integrity, docs vs code) over the working-tree diff, then two skeptics per
blocker/should-fix finding. **No reviewer ran cargo**, so nothing was
reproduced by executing it at review time. 17 raw findings were confirmed and 0
refuted, consolidated into 1 blocker and 12 should-fix rows. Zero refutations is
weak evidence (the skeptics were shown the claim), so the fix pass re-checked
each fix by **mutation**: restore the old code, watch the new test fail for the
stated reason, restore the fix, watch it pass.

## Resolution

| # | Finding | Fix | Evidence |
| --- | --- | --- | --- |
| B1 | Autocomplete redaction used `starts_with` on the whole attribute, so `billing cc-number` and `section-pay cc-csc` leaked | `attribute_is_sensitive` tokenises `autocomplete` on whitespace and matches any token | 4 unit tests in `dom.rs`; the live lock test now covers token lists. Against the pre-fix `dom.rs` it fails with "Billing card must not carry a live value" |
| S1 | `current-password`/`new-password` and reveal-toggled password fields leaked | Both autocomplete values redacted; `name`/`id` containing `password`/`passwd`/`pwd` redacted; a node with no attribute record fails closed | Same tests. **Residual gap** under [Accepted](#accepted-and-deferred) |
| S2 | Live `value` unbounded | Capped at 200 chars on a char boundary, marked with `…` | Unit test plus live test; pre-fix fails "500-char value must be capped" |
| S3 | `prefers_dom_click` tested the box edge, so a partly scrolled tall element lost its trusted mouse event | Tests the click point against the viewport (`viewport_size()`), including the right and bottom edges | Unit tests with a 1280×800 viewport; three live tests assert `isTrusted`/`pointerdown` for in-viewport and partly scrolled elements. Pre-fix routing gave `click:false` with no pointerdown |
| S4 | `this.click()` is undefined on SVG/MathML; error surfaced as bare "Uncaught" | Dispatches a `click` event when `click` is absent; error reason comes from `exception.description` | Live test on an off-screen SVG node; pre-fix "DOM click threw …: Uncaught" |
| S5 | `Meta+a`/`Control+v` were silent no-ops reporting success | New `bu-cdp/src/keys.rs` attaches Chromium editing commands (`selectAll`, `copy`, `cut`, `paste`, `undo`, `redo`) to the main key's down event | `press_key_accelerators_run_their_editing_command` asserts the field's resulting value; pre-fix `Control+a` left `"old valu"` |
| S6 | No `char` event (Enter did not submit); virtual key codes for 10 keys only | `text` on printable keys and Enter; virtual key codes for navigation, editing and function keys; side-specific modifier `code` | Live tests: Enter submits the form (title changes), Home/Delete move and delete, printable insertion. All fail on the pre-fix code |
| S7 | `paste_at_coordinates` cleared the field before the clipboard write, so a failed write wiped it | Order is click → save clipboard → write → clear+paste → restore clipboard; a failed write leaves the field intact | Test asserts the field still reads `"old value"` after the failure; pre-fix it was `""` |
| S8 | Fail-closed policy test did not cover the new mutating commands | `forbidden_first_page_and_mutations_fail_closed` now drives `type_at`, `paste_at`, `press_key` | Removing the `guard_active_url()` call from each handler in turn turns the test red naming that action; restored and green |
| S9 | Hand edit to a generated skill file | `skills/browser-use/SKILL.md` reverted to its pre-WIP content; the note lives in `docs/usage/tools/mcp-multi-agent-setup.md` with its unverified status stated | `git diff 3c6a553e7^ -- skills` is empty. See the sync-check caveat below |
| S10 | Changelog claimed "binary rebuilt and installed" and "169 tests"; repeated a paragraph | Duplicate removed; claims relabelled as the author's and not reproduced | Read the file |
| S11 | Deploy probe asserted 19 tools; the test asserts 21 | Deploy practice, `docs/index.md`, architecture pages and setup doc updated; the practice names the test to change together with the count | `grep` for current-state "19 tools" finds only dated historical records |
| S12 | Only one slice had a board row | Board rows added for the Flutter coordinate typing slice, the upstream live-form-state port and this fix pass; HANDOFF updated | Read the board |

The also-flagged items from the original "not verified" list:

- **Clipboard hygiene.** The grant is now scoped to the page origin and reset
  after every use, the pasted text no longer stays in the clipboard (the previous
  contents are restored; if they could not be read the clipboard is cleared), and
  a test asserts the permission is back to default and the clipboard holds no
  pasted text afterwards.
- **`browser_type` index XOR coordinates** is enforced with `INVALID_PARAMS`;
  a test covers index plus coordinates.
- **Modifier aliases** reach the page under their canonical name (`Cmd` →
  `Meta`); `modifier_aliases_reach_the_page_under_their_canonical_name` asserts
  the emitted `key` and `code`.
- **"Byte-identical to Python"** is corrected to 15 of Python's 16 tools:
  `browser_type` extends Python's schema (Python requires `index` and has no
  coordinates).

## Nits

All 18 were triaged.

| # | Nit | Disposition |
| --- | --- | --- |
| 1 | No content quad fell through to an unconditional DOM click | **Fixed.** A missing box re-checks the live selector map and refuses a node that is hidden or gone. Live test `click_by_stale_node_id_refuses_a_button_hidden_since_the_snapshot` failed first (the hidden "Delete account" button was clicked), then passed |
| 2 | Docs said off-viewport but right/bottom edges unchecked | Fixed with S3 |
| 3 | Skill stated Flutter activation as fact | Fixed with S9; the usage note says it is unverified |
| 4, 9 | Modifier alias sent verbatim; alias test could not see it | Already fixed; the existing alias test asserts the emitted `key` |
| 5 | XOR not enforced | Fixed |
| 6 | Toggle detection case-sensitive | **Fixed** via `is_toggle_type`. `type="CHECKBOX"` was read as a text input; a unit test and a live test fail with the case-sensitive version |
| 7 | `value` echoes email/credential-like text that `masked_typed_text` masks | **Accepted.** The two are different surfaces: `masked_typed_text` masks what the agent typed in a tool echo, while `value` reports the page's own field content like `text` and `href`. Redaction is by field class (password, file, hidden, card, one-time code), not by content shape, so an email in a plain field is visible |
| 8, 16 | Duplicated paragraph; zero-width space | Fixed |
| 10 | Chrome-free tests gated behind `live-chrome` | **Deferred.** Structural; the pure logic added in this pass (`keys.rs`, `dom.rs`, `geometry.rs`) has Chrome-free unit tests |
| 11 | Icon-label test partly passes before the diff | **Deferred.** The assertion that matters (the label fallback) is unchanged; tightening it needs a fixture redesign |
| 12 | Checked-state tests lacked an all-unchecked page and mixed-case type | **Fixed.** `selector_map_reports_checked_false_when_nothing_on_the_page_is_checked` passes, and fails against a case-sensitive toggle check. An all-unchecked page already reported `checked: false`, so the `None`-vs-empty distinction is not a gap |
| 13 | Coordinate-mode error branches and echo masking untested | **Partly fixed.** XOR rejection is tested; the stray-single-coordinate branch and echo masking in coordinate mode are **deferred** |
| 14 | `prefers_dom_click` tests pinned two points | Fixed with S3 (a table of cases against a fixed viewport) |
| 15 | Stale "16 byte-identical" comment | Fixed |
| 17 | Plan's verification commands did not run the checked-off test | Fixed; the plan and changelog now run `--features live-chrome click_` |
| 18 | Unexplained "mu H5" | Fixed; reworded without the identifier |

## Accepted and deferred

- **Reveal-toggled password with no tell (narrowed 2026-10-15).** A password
  field whose `type` was flipped to `text` and that carries no password-like
  `name`, `id`, `placeholder` or `aria-label` and no password `autocomplete`
  cannot be recognised from the snapshot, so its live value is reported. Two
  changes narrow it: those four attributes now count as tells, and a per-tab
  memory of backend node ids that ever looked sensitive keeps a field redacted
  after a reveal toggle flips its type (cleared on `navigate()`; a stale id can
  only over-redact). What remains is a field never seen as a password that has
  none of those tells. Closing that would mean redacting every text field, which
  defeats the feature.
- **`BringToFront` in the clipboard grant.** It raises the tab. Chromium requires
  a focused document for the clipboard APIs, so this stays.
- **`browser_click` keeps Python's fall-through** (index with both coordinates
  uses the coordinates). Only `browser_type` was made strict.
- **Nits 10, 11 and the coordinate-mode remainder of 13** (above).
- **MBP-local itself.** Its tree is still dirty and its installed binary predates
  this work. Nothing was deployed or changed on any host.

## What this does not prove

Items below were open when this record was written; the first four were
revisited on 2026-10-15, see
[the gap-closing changelog](../changelogs/2026-10-15-close-review-gaps.md).

- **Live Flutter (closed 2026-10-15).** The DOM-click path was proven against a
  1×1 off-screen button and an SVG node, then against a locally built Flutter Web
  3.44.8 login page: the placeholder click exposed `Email`, `Password` and
  `Log in`, and `Log in` changed the app's state. One Flutter version, headless
  only, a debug build from this tree rather than an installed binary. The page
  also showed that `browser_type` by index silently dropped text, now
  [its own issue](2026-10-15-flutter-index-typing-drops-text.md), fixed locally.
- **The skill sync CI job (resolved 2026-10-15).** `scripts/sync_browser_harness_skill.py
  --check` fetches upstream `browser-harness` `main` live; it exited 1 with a
  298-line diff, none of it from this fork. The script was re-run and both
  generated copies were regenerated: `--check` exits 0 and the CI docs test
  passes. This is not durable: the job drifts red again whenever upstream moves.
  It runs only on pushes to `main`, pull requests touching the listed paths, and
  manual dispatch.
- **The 10-minute stall (not a proven hang; a real bug was found).**
  `forbidden_first_page_and_mutations_fail_closed` produced no output for over ten
  minutes at 0% CPU under machine load ~21. It was never reproduced as a true
  hang: one 75 s stall under ten CPU hogs with the test thread idle, and none in
  18 further hogged runs. Reproducing under load did expose a separate,
  deterministic failure: `state()` read the title through a cached execution
  context that navigation had already destroyed (`Cannot find context with
  specified id`), failing the test in 4 of 8 hogged runs. `state()` now issues a
  raw `Runtime.evaluate` without a context id and the test passes 10 of 10 under
  the same load. The test also has a 120 s wall-clock bound so a stall fails and
  names itself. About fifteen other `page.evaluate` call sites share the latent
  race and were left unchanged; see
  [the learning record](../learning/2026-10-15-pinned-execution-context-races-navigation.md).
- **Final gates are green on this host only.** `cargo fmt --check`, clippy
  `-D warnings` (default and `live-chrome`) and `cargo test --features live-chrome
  -- --test-threads=1` all exit 0: **202 passed, 0 failed** (174 before the first
  fix pass, 200 after it, 202 with the two tests added for the gaps). No other
  host ran them.

[Issues index](README.md) · [Landing commit context](../HANDOFF.md)
