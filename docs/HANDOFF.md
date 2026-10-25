# Handoff

## Current local sync (2026-10-25)

Codex registration was repaired after the owner reported that `/mcp` omitted
browser-use. The default user config now enables the absolute installed
`browser-use-rs --mcp` command with headless mode. Its read-back command passed
MCP initialization, 21-tool discovery, navigation/state and browser closure.
Existing Codex settings were preserved and config permissions remain 0600.
**Restart the existing Codex client to load the new registration.** No live
model call was made, and the build cache remains absent.

Completed: the fork's `franky-rust` branch contains merge `0d9e428e8` through
upstream `3df5806a0` (Python 0.13.13), including the original local fixes.
That implementation commit is published to the owner's fork. The local
`~/.local/bin/browser-use-rs` was rebuilt from `0d9e428e8`, installed with a
fresh inode and valid signature, and matched its build artifact hash.
Sequential installed MCP initialization listed **21 tools**; live native input,
disabled-choice, privacy and screenshot smoke checks passed. Reconnect existing
MCP clients to load the new executable.

At the owner's request, `cargo clean` subsequently removed `rust/target`,
reclaiming about 32.5 GiB. The independent installed copy retains its recorded
hash, valid signature and working CLI. Rebuild before comparing against a
build-tree artifact again.

All 243 serial all-feature Rust tests, both Clippy configurations, formatting,
the release build and 51 focused Python merge contracts passed. This clone's
`origin` now uses existing SSH authentication for the same owner fork after
HTTPS could not prompt for credentials. Other hosts retain their separate
deployment tasks.

See the [completed plan](plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.md)
and [change/verification record](changelogs/2026-10-25-upstream-rust-sync/index.md)
for scope, explicit Rust gaps and rollback evidence.

## What the team is trying to finish

This fork (`franky-rust`) maintains a Rust reimplementation of browser-use's MCP
server, `browser-use-rs`, which is the browser-automation backend for every
coding agent on two developer machines. Two threads are live:

- **The Rust rewrite itself** — the long-running port, governed by
  [2026-07-05-rust-rewrite](plans/2026-07-05-rust-rewrite/index.md).
- **Operating the deployed server** — keeping the binary and its MCP
  registrations correct across both hosts, governed by
  [usage/tools/mcp-multi-agent-setup.md](usage/tools/mcp-multi-agent-setup.md)
  and [practices/deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md).

Current status for every task is on the board:
[plans/README.md](plans/README.md).

## Earlier milestones and deployments

The whole-repository hardening goal is complete with honest environment blocks.
RST-04 Bedrock Converse typed tools, streaming, retries, timeouts, and usage are
implemented and fixture-proven (`bu-llm --features bedrock`: 70 tests, twice).
Keep-alive `stop()`/`start()` rebinds session handlers. Cookie scope rejects
single-label suffix domains. Setup creates and verifies one repo-root
environment. See the [hardening plan](plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md).
At the 2026-09-16 deployment, source `e35b0a882` was installed on local,
MBP2 and franky-frank. The current local installation is recorded above. Each host
rebuilt `browser-use-rs` from that commit, installed a fresh inode, and
answered a sequential MCP `initialize` + `tools/list` with 19 tools (exec
check exit 1, not 137).

Python Ruff, format, Pyright, focused regressions, package build, release
contracts, and `tests/ci` (1116 passed, 34 skipped) pass. Rust format, Clippy
`-D warnings`, default tests, two agreeing env-isolated all-feature workspace
runs (actor 6, agent 9, CDP 25, LLM 70, MCP 37), and live CDP/MCP attach/policy
tests pass. Docker daemon execution and real external provider credentials
remain unavailable locally and are recorded as blocked rather than passed.

Recent slice (2026-09-16): Bedrock Converse tools/streaming fixtures, keep-alive
event-bus rebind, cookie public-suffix rejection, setup-script environment
proof, DevTools error redaction, and existing-tab attach. Pushed `e35b0a882`
and installed on local, MBP2, and franky-frank; trees are at `7763dece4`. The
MCP fixture uses the explicit `BROWSER_USE_LLM_*` environment surface.

Recent slice (2026-10-15): MBP-local is a **fourth host** outside the three above.
It held 15 files of uncommitted, unpushed work (DOM-click offscreen, Flutter
coordinate typing, live form state). It was pulled here byte-identically (per-file
sha256) and landed as `3c6a553e7`. A pre-commit review found **one blocker
(autocomplete redaction bypass) and 12 should-fix items**; all are fixed with
mutation-checked tests, and the 18 nits are triaged, in
[the review findings](issues/2026-10-15-mbp-local-wip-review-findings.md). The
fixes are on `franky-rust`. **At that point nothing was deployed:** MBP-local's own tree was still
dirty and its installed binary predates the work. The tool count is now **21**
(15 Python-identical, `browser_type` extended with coordinates, 5 additions), and
the probe in [deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md) expects
it. The four gaps that review left open were closed the same day, see
[the changelog](changelogs/2026-10-15-close-review-gaps.md): the live DOM-click
check passed on a real Flutter Web page, the generated skill files were re-synced
(they drift again whenever upstream `browser-harness` moves), the reveal-toggled
password redaction gap was narrowed, and the policy-test stall turned out to be a
real stale-execution-context race in `state()`, now fixed. That live check also
found that `browser_type` by index silently dropped text on Flutter fields. On
2026-10-16 the cause was measured (Chromium's initial tab is unfocused; the server
now emulates page focus in browsers it launched or that are headless) and the tap-first workaround was replaced, see
[the change record](changelogs/2026-10-16-page-focus-and-flutter-gaps.md) and
[the issue](issues/2026-10-15-flutter-index-typing-drops-text.md). The same change
made `browser_scroll` reach Flutter's internal lists and moved every script
evaluation off the pinned execution context. Real Flutter 3.44.8 verified
(CanvasKit and WasmGC, headless); deployment was pending then. The current
local install includes these fixes.

**The LLM environment surface changed on 2026-08-27 and is a breaking change.**
Configure the model with `BROWSER_USE_LLM_BASE_URL`, `_API_KEY`, `_API`
(`openai-responses` | `openai-chat` | `anthropic-messages` | `bedrock`), and
`_MODEL`. `OPENAI_*`, `ANTHROPIC_*`, `MODEL_PROVIDER`, `BROWSER_USE_OPENAI_API`,
and `BROWSER_USE_API_KEY` are no longer read at all. All 10 agent configs on both
hosts are migrated (backups at `*.bak-llmenv`); any *new* config must use the new
names or the server will refuse to start with a message naming the variable.

Note for any **third** clone: the guardrail's `core.hooksPath` is per-clone local
config and does **not** arrive with a pull. The hook files will be present but
inert until someone runs `git config core.hooksPath config/git-hooks`.

Two divergences a newcomer would otherwise trip over:

- The hosts deploy the binary **differently**. The primary symlinks
  `~/.local/bin/browser-use-rs` into `rust/target/release/`; the secondary keeps
  a file copy. Only the copy can go stale, and git reports nothing when it does —
  see [the drift issue](issues/2026-08-26-installed-binary-drift-from-build-tree.md).
  Hash the deployed file against its own host's build tree before assuming a
  sync is done.
- The secondary host's Kimi entry attaches to a running Chrome over an
  **ephemeral** CDP port rather than launching its own. It works today and is
  deliberate, but it breaks whenever that Chrome restarts.

## What needs to happen next

1. **Seed the secondary host's Keychain from a local terminal.** Its
   `~/.config/browser-use/.env` holds the key in plaintext because a
   non-interactive SSH session cannot unlock the login keychain. From a terminal
   *on that machine*:
   `security add-generic-password -U -a "$USER" -s browser-use-llm -w 'sk-…'`,
   then delete the `BROWSER_USE_LLM_API_KEY` line from its `.env`.
   Done when: that host answers a `browser_extract_content` call with no key in
   any file.

2. **Verify the Anthropic Messages path against a real endpoint.** It is
   implemented and unit-tested against a mock, but no Anthropic key was
   available here, so it has never spoken to `api.anthropic.com`. Set
   `BROWSER_USE_LLM_API=anthropic-messages` with a real key and run
   `browser_extract_content` once.
   Done when: a live Anthropic call returns an answer, or the gap is fixed.

3. **Decide on one binary deploy layout across both hosts.** The primary
   symlinks `~/.local/bin/browser-use-rs` into `rust/target/release/`; the
   secondary keeps a file copy. The symlink cannot drift but breaks on
   `cargo clean`; the copy survives a clean but goes stale silently, which is
   what already bit us once.
   Done when: both hosts use the same layout and
   [deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md) states which
   and why.

4. **Give the secondary host's Kimi entry a stable browser.** It attaches over
   an ephemeral CDP port that dies whenever that Chrome restarts.
   Done when: the entry either launches its own browser or points at an
   endpoint that survives a restart.

5. **Resume the Rust rewrite** at whatever its plan's tracker names as the next
   step. Done when: that step's own exit evidence is satisfied.

6. **Expose the current Chrome DevTools endpoint** when live attachment is
needed. Start Chrome with remote debugging, set `BROWSER_USE_CDP_URL`, then run
the sequential attach probe in the deployment plan. See
`issues/2026-09-16-current-chrome-devtools-endpoint.md`.

7. **Deploy the 21-tool build on remaining hosts.** Local installation passed
   on 2026-10-25; other hosts require their own scope and probes. Follow
   [deploy-browser-use-rs.md](practices/deploy-browser-use-rs.md) on each host (the
   probe expects 21 tools) and reconnect the MCP client. The live Flutter check and
   the [page-focus fix](changelogs/2026-10-16-page-focus-and-flutter-gaps.md)
   both passed on a local debug build (2026-10-16).
   Done when: the deploy probe passes on each host.

## How to pick up the work

Update the row on [plans/README.md](plans/README.md) and the detailed plan
together whenever work starts, blocks, or completes — the board owns cross-task
status, the plan owns scope and decisions, and a tracked plan's `.track.yaml`
owns step-level state.

Constraints to preserve:

- **The routers now conflict with upstream by design.** `AGENTS.md` (1070 → 100
  lines) and `CLAUDE.md` (211 → 108) had their embedded documentation extracted
  into [`docs/usage/library/`](usage/library/README.md) and
  [practices](practices/2026-08-26-python-code-style-and-change-workflow.md) on
  2026-08-26, at the owner's direction. When merging `upstream/main`, resolve by
  keeping the router thin and applying upstream's content change to the matching
  `docs/usage/library/` page — do **not** paste the handbook back into the
  router. For any *other* inherited file (CI, linters), check
  `git diff upstream/main -- <file>` and ask before restructuring. See
  [the router lesson](learning/2026-08-26-fork-router-editable-region.md).
- **Never `cp` over the live binary** — macOS SIGKILLs it. Fresh inode, then
  re-sign; the deploy practice has the sequence.
- **Secrets live in `~/.config/browser-use/.env` (0600), not in agent configs.**
  Settings resolve process env → `.env` → Keychain (API key only). Do not put a
  key back into an agent's MCP config; and re-check `stat -f "%Sp" ~/.grok/config.toml`
  after any `grok mcp add`, which recreates it world-readable. See
  [the exposure issue](issues/2026-08-27-agent-config-world-readable-secrets.md).
- **Redact secrets in records.** Agent MCP configs hold live API keys and
  gateway hosts. Note that `qoder mcp get <name>` prints them in plaintext;
  prefer `qoder mcp list`.
