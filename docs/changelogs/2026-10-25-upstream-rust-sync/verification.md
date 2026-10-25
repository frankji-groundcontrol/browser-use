# Verification evidence

## Before the ports

- Installed MCP initialized and listed 19 tools; local source defined 21.
- Default Rust workspace: 129 tests passed.
- Private recovery snapshot captured all 18 pre-existing dirty files.

## Focused contracts

- Agent live-form observation: null-value RED, then GREEN through an actual
  local browser and captured model request; sensitive value absent.
- Anthropic simple and typed image requests: two RED/GREEN slices; final
  crate suite 65 passed, Clippy and formatting passed.
- DOM: 12 CDP DOM unit tests and 7 extraction unit tests passed, including
  the review regressions for image authority and CSS custom declarations.
- Merge conflict checks: 31 focused Python tests passed; another 20 session,
  live-value, style and image-serializer tests passed. Syntax/Ruff PLE passed.
- Native date/time and disabled-option MCP contracts: both reproduced RED,
  then GREEN. Five native types fill; invalid native values and disabled
  choices preserve existing form data.
- Launch security: two-origin CORS RED/GREEN. Empty headless value RED/GREEN.
- Cached-page recovery, abrupt reset and clean close: reproduced as distinct
  failures, then GREEN. The validated-handle HTML and snapshot regressions
  passed; all 11 serial actor tests passed.

## Release and delivery

The first complete integrated run passed formatting, 239 all-feature serial
Rust tests, default and all-feature Clippy with warnings denied, and the release
build. One combined review then identified three missing cases (validated-page
identity during recovery, credential-bearing image authority, and CSS custom
properties). All three repairs passed their regressions. The fresh complete acceptance run
after those repairs passed all five gates:

- `cargo fmt --all -- --check`
- `cargo test --locked --workspace --all-features -- --test-threads=1`: **243 passed, zero failed or ignored**
- Default-feature workspace/all-target Clippy with `-D warnings`
- All-feature workspace/all-target Clippy with `-D warnings`
- `cargo build --locked --release -p bu-core`

The suite totals are actor 11, agent 10, CDP 70, core 11 plus one integration,
DOM 7, LLM 72 and MCP 61. The 51 focused Python merge contracts are also green.

The final audit covers 198 paths and 12 known untracked files. No protected
files or real credential/provider pattern hits were found. Four synthetic
credential URL cases in the unchanged upstream security test are explicitly
accounted for; their destination hosts are the fixture domains. All 183 local
document links resolve; whitespace checks pass.
The only file above 1 MiB is the already-tracked lockfile, which shrank.
Implementation commit/push, fresh-inode signed install, sequential MCP smoke
and installed/build hash comparison passed. All 12 previously visible untracked
files are now accounted for in the implementation commit; the final closure
changes only tracked records.

No live model/provider or paid cloud verification is claimed. Other hosts and
the global Python CLI are unchanged by this task.

## Original-file preservation

All 18 original dirty files remain present. Eight retain their exact original
hash (including both original untracked records); one usage record retains its
full original content with an appended section. The other nine paths are four
Rust files integrated with the new ports and five handoff/index records updated
for this task. The private recovery snapshot remains available; existing
focus, Flutter, redaction and execution-context regressions remain in the suite.

## Published source and installed executable

- Owner fork branch: `franky-rust`; published implementation `0d9e428e8994510484c553c86f58fba290d72d39`.
  Its two parents are the starting fork and the frozen upstream target.
- HTTPS push could not prompt for credentials in this session. Existing SSH
  access to the same fork succeeded; this clone's `origin` transport now uses
  SSH. The owner/path and branch are unchanged. Upstream is only merge input.
- Installed `~/.local/bin/browser-use-rs` has a fresh inode and valid ad-hoc
  signature. Build/installed SHA-256: `fcc4da0991569e772b222840968132b2c67770fad94c054da7ef547f8a3c95a0`.
- Rollback copy: `~/.local/bin/browser-use-rs.prev-20261025-182510-570759`. It retains the previous file hash.
- CLI version/help passed. The Rust package reports `0.1.0`; this is distinct
  from the merged Python source version `0.13.13`. Uninitialized MCP EOF exited
  1, with no macOS signature-related kill.
- Sequential installed MCP: server `browser-use`, 21 tools, native date fills,
  invalid date preserves its previous value, disabled optgroup choice fails,
  sensitive form value stays absent, checkbox state is present, JPEG capture
  succeeds, and `browser_close_all` closes the owned test browser.
- Smoke used a fresh headless browser and fixture-only model settings; zero
  model calls. Existing user browser profiles and agent registrations were not
  used. Existing MCP processes need reconnection to load the new executable.

The following documentation closure commit changes no production source.
Final clean-tree and owner-fork SHA checks are the publication exit gate.

Final closure audit: 198 changed paths, 184 valid local document links, no
untracked files, no new large artifact or privacy-pattern hit, and no source
change after the accepted release run. Installed and build hashes still agree.

## Requested cleanup after installation

`cargo clean` with the explicit `rust/target` directory removed 239564 files
(47.1 GiB logical size). The target directory is absent; available disk space
increased by approximately 32.5 GiB. The installed executable retains
its recorded SHA-256 and valid signature, and `--version` still succeeds.
The separate rollback copy remains in place. No rebuild or full-suite rerun
followed cleanup, since those actions would recreate the removed cache.

## Codex client registration repair

The user's `/mcp` output and effective `codex mcp list --json` both omitted
browser-use. The binary's recorded hash and version still matched. A private
backup preceded registration; Codex CLI 0.164.0 normalized three unrelated
fields, so the final edit retained the complete original bytes and appended
only an enabled absolute stdio command plus `BROWSER_USE_HEADLESS=true`.
Parsed settings outside browser-use are unchanged; config mode is 0600.

Read-back from the user's workspace confirms enabled stdio registration.
Launching that exact command and args passed initialization (server browser-use),
21-tool discovery, local navigation, state capture and browser closure. No
credentials were copied into the new entry, no model was called, and target
remains absent. The existing Codex session's tool inventory has not reloaded;
reopening the client is the remaining activation step, not a claimed UI proof.
