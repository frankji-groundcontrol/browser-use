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
Commit/push, fresh-inode signed install, sequential MCP smoke, installed/build
hash comparison and final untracked audit remain pending.

No live model/provider or paid cloud verification is claimed. Other hosts and
the global Python CLI are unchanged by this task.

## Original-file preservation

All 18 original dirty files remain present. Eight retain their exact original
hash (including both original untracked records); one usage record retains its
full original content with an appended section. The other nine paths are four
Rust files integrated with the new ports and five handoff/index records updated
for this task. The private recovery snapshot remains available; existing
focus, Flutter, redaction and execution-context regressions remain in the suite.
