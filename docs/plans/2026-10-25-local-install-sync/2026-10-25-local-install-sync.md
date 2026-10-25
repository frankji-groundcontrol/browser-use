# Rust upstream sync and local installation

Date: 2026-10-25
Owner: Codex
Status: complete

## Outcome and scope

Compare the existing `franky-rust` checkout with both `origin/franky-rust`
and `upstream/main`; translate new applicable upstream behavior into Rust,
merge and publish the fork, and rebuild and install the local Rust MCP binary. Preserve all pre-existing uncommitted work. This is a
standalone Git boundary; the separate workspace repository is untouched.

## Restored state and decisions

- Restored `docs/HANDOFF.md`, the task board, deployment practice, and existing
  deployment YAML. Existing multi-host work is outside this local request.
- Starting HEAD: `e49186db1`; tracking branch: `origin/franky-rust`.
- User correction: Rust fork only. The Python CLI installation is out of scope.
- User expanded scope: inspect new upstream features, port missing Rust behavior,
  merge it into the Rust version, and update the fork remote.
- Initial dirty-file contents and the full visible untracked-file list are
  preserved in a private temporary recovery directory before any sync.
- The existing untracked files are two dated documentation records. The final
  audit will compare their hashes and account for this task's new plan files.
- Engineering review: use the existing install/deploy paths, an intentional upstream merge,
  immutable lockfiles, and a fresh signed binary inode. Review the upstream contract and all Rust callers before each port. Use
  behavior-level TDD for actual Rust changes; keep refutations evidence-backed.

## Steps and acceptance evidence

- [x] Restore instructions and task state; preserve the dirty-file baseline.
- [x] Fetch origin and upstream; compare branch ancestry and upstream feature changes.
- [x] Inventory applicable features and review the engineering scope.
- [x] Port missing Rust behavior with regression tests; merge upstream Python sources.
- [x] Run required Rust release gates; rebuild and install the MCP binary.
- [x] Confirm CLI startup, sequential MCP initialization and tools/list,
      original-file preservation, links, and final Git status.
- [x] Close this plan and YAML, task board, dated changelog, and handoff.

## Constraints and privacy

Never read secret files or emit provider configuration. Keep all original
source edits. Commit and push the reviewed update to the existing fork branch; do not touch
other hosts. Preserve the previous
installed binary for rollback. Temporary environments and build artifacts
remain ignored. The parent workspace's untracked audit remains current for
this task because no paths in that workspace are changed.

## Evidence

Both remotes were fetched and compared below. All ports, review repairs, release gates, fork publication and local installed smoke checks are complete.

Learning: a directory named browser-use may be a custom Rust MCP fork; restore
its handoff and honor the user-selected implementation before changing tools.

## Remote comparison checkpoint

- `HEAD` and `origin/franky-rust`: `e49186db131955759a13e9b7991ad72c4640133f`.
- Fetched `upstream/main`: `3df5806a0d097a1546d92c49bcf1b28c9972c3bd`.
- Common base: `28670f720f63cc5f525a2acd6d6072867689ab68`.
- 361 missing upstream commits (100 first-parent commits), 157 changed files.
- Smallest end-to-end baseline: installed MCP initializes and lists 19 tools;
  the dirty local source defines 21 tools. No model request or cloud browser used.
- Engineering review: independently map DOM, lifecycle/security, and LLM changes;
  coordinator maps MCP and interaction integrations. Read-only investigations
  share frozen Git refs; no worker edits or browser launches during the map.
  Return condition: contract, Rust counterpart, applicability, and test evidence
  for each change group. Production edits start only after the map is integrated.

## Engineering scope checkpoint

The upstream MCP server adds no new browser tool names in this window: changes
are SDK registration/error API migrations, already covered by Rust's rmcp and
error contracts. New OpenAI/Claude Python SDK adapter classes and Bash executors
are Python integration surfaces, not APIs consumed by the Rust MCP server.
Their browser behavior is checked against existing Rust operations; absent
whole subsystems remain explicit gaps in the final applicability matrix.

Confirmed ports so far:

1. DOM image context: preserve existing labels, expose bounded self/descendant
   image clues, normalize data URL controls, bound attribute and traversal work.
2. Hidden code extraction: normalize display:none styles in the shared parser.
3. Agent observations: carry already-redacted live value and checked state.
4. Anthropic images: one data URL parser for mixed-case scheme/MIME across
   simple and typed requests, preserving remote URL sources.
5. Lifecycle/config investigation: cached-page recovery, opt-in launch-security
   flags, and empty headless environment values require contract checks.
6. Coordinator input checks: existing plus-key and unknown-tool tests already
   cover those upstream changes; native input and dropdown validation are being
   compared before any production edit.

Architecture remains existing crate boundaries. No new dependencies or Python
CLI install. Workers own disjoint Rust files; coordinator alone owns Git and
durable records. Behavior-level red/green tests precede implementation. Final
serial live-Chrome gates protect integration and original local fixes.

## Implementation handoff checkpoint

Baseline default Rust workspace tests: 129 passed, zero failed.
The upstream merge is intentionally uncommitted. Six inherited files have
content conflicts: publish workflow, Actor element/mouse/page, browser session,
and extension-config tests. Existing dirty paths did not overlap incoming
changes; these conflicts are between earlier committed fork hardening and new
upstream implementations. Resolve by preserving behavior, not by choosing one
whole side indiscriminately. Re-run retained regression tests after resolution.

Independent implementation ownership:

- dom_map: CDP DOM image context and shared HTML-to-Markdown hidden-code parser;
  no CDP session/lib or agent edits. Pure red/green tests, no Chromium launch.
- llm_map: shared Anthropic image conversion across simple/typed requests and
  tests. No live provider requests, no external dependencies.
- lifecycle_map: Rust actor recovery and CDP session/launch/discovery modules
  plus their tests. Owns the serial live-browser lane until it returns. Avoid
  speculation: clean-close repair must first fail against a real fixture.
- Codex: upstream conflict resolutions, agent observation projection, input
  applicability/regressions, all Git, release gates, records, publication/install.

The existing Rust MCP API is the translation target. Broader upstream-only
subsystems (SDK adapters/Bash, native Gemini/Ollama factories, agent file export,
saved-history maps, cloud provisioning) are recorded as unsupported rather than
misrepresented as already ported. The fork retains the merged upstream Python
implementations for these APIs.

## Red/green checkpoint and dependent handoff

- Agent observation regression failed with `value = null` instead of Shanghai;
  adding value/checked to the existing observation made the same real-browser /
  recorded-LLM-request test pass. Sensitive values stayed absent.
- Merge conflict contracts: 31 focused Python tests passed (Actor lifecycle,
  mouse scroll anchors, extension config/download timeout, release version gates).
- Project lock refreshed for merged Python 0.13.13 dependencies; project venv
  refreshed only for merge verification. Global Python CLI is unchanged.
- Two upstream Actor contracts reproduced as MCP failures before implementation:
  native date input returned success but retained an empty value; choosing a
  disabled optgroup option returned success and changed the form. New MCP tests
  are `typing_fills_native_date_and_time_inputs` and
  `selecting_a_disabled_option_keeps_the_existing_value`; both exited 101.
- lifecycle_map now receives these two proven input repairs in its owned CDP /
  actor files. Preserve current form data when rejecting an invalid native value;
  use the native setter and real input/change events for date/time variants,
  and reject disabled selects/options/optgroups before changing selection.
- Coordinator returned the exclusive live-browser lane to lifecycle_map.

Detailed [upstream applicability matrix](2026-10-25-local-install-sync.upstream-matrix.md).

## Accepted recovery contract

Clean WebSocket Close was reproduced as a distinct failure after abrupt-reset
handling passed. Recover when the next command checks the connection; do not
introduce an idle heartbeat across every long-lived MCP process. A bounded
browser-level probe may add a round trip before cached-session reuse. Keep
`is_healthy` documented as cached health, invalidate page and selector state
when reconnecting, retain owned-versus-attached lifecycle semantics, and never
replay an input action whose result is ambiguous.

The user was offered the optional API-expansion choice. No expansion answer has
arrived; work continues with the existing Rust MCP surface as stated publicly.

## Integrated-port checkpoint

DOM owner returned 11 passing CDP DOM tests and 6 extractor tests, with all seven
new behavior tests observed RED first. LLM owner returned 65 passing tests and
Clippy/format checks after both public request methods failed then passed their
image contracts. Coordinator's agent observation RED/GREEN and all 51 focused
Python merge contracts passed. All six merge conflicts are resolved and staged.
Lifecycle owner is completing the final native-input contracts; command-triggered
clean-close detection and abrupt reset recovery are green against local fixtures.

## Source freeze for integration gates

All workers returned and the serial live-browser lane is free. Native date/time
MCP test passes for five input types; disabled choices fail without mutation;
invalid native dates preserve the previous value. A further RED/GREEN security
contract preserves clear -> URL-policy check -> type ordering when an input
handler navigates. CDP session fixtures passed CORS opt-in, attached/default
security, abrupt transport loss and next-command clean-close detection.
New focused modules are `bu-cdp/src/input.rs` and `session_tests.rs`.
Source is frozen for one combined review and integrated feature gates.

One independent combined review is assigned to llm_map against the frozen
working tree, with the original private dirty baseline as context. No review
edits or test/browser processes are allowed; return only concrete introduced
bugs and evidence. Coordinator runs the integrated serial gate lane in parallel.

## Privacy and artifact audit checkpoint

The 196 changed/new owned paths contain no protected secret files, credential,
database-URI or provider-ref pattern hits. All 12 visible untracked files are
accounted for as original records, dated task records, or two Rust source modules.
There are no nested gitlinks. The sole file above 1 MiB is the already-tracked
`uv.lock` (1246863 -> 1237639 bytes); it is required dependency
source, not a new generated binary. The initial generic size assertion was
narrowed to distinguish this existing lockfile from new large artifacts.

## Single review findings and repair checkpoint

The combined review found three concrete issues; repair them before delivery:

1. Recovery between URL validation and the next page fetch can adopt an
   unvalidated tab. Return and use the exact validated page handle throughout
   each command; preserve post-clear policy checks and no input replay.
2. Image filename extraction can mistake URL authority/userinfo for a filename
   when the path is empty. Parse URL paths and omit empty paths.
3. Hidden-code substring matching wrongly treats a custom property such as
   `--display:none` as the actual display declaration. Match the declaration.

The final wait returned successful completion of all five release gates:
239 tests, formatting, default/all-feature Clippy, and the release build. An
intermediate lack of new output plus a process snapshot was initially mistaken
for a waiting output pipe; no wrapper process was found or terminated. The
completion result supersedes that inference. These gates precede the review
repairs, so source changes still require a fresh integrated acceptance run.

Repair ownership: dom_map handles findings 2/3 in its existing files using pure
RED/GREEN tests; lifecycle_map handles finding 1 in actor code and owns the live
browser lane. Coordinator alone owns Git, gate runner and records.

## Review repairs frozen

All three findings are now reproduced and repaired. The image-authority test
failed before URL-path parsing and now passes; the inline custom-property test
failed before declaration parsing and now passes. CDP DOM tests: 12 passed;
shared extractor tests: 7 passed. The attached-Chrome recovery regression first
returned denied page content after a second reuse probe recovered to another
tab. The exact validated page handle now serves the entire operation; both
HTML and snapshot variants pass. The full serial actor suite passed 11 tests.
Selector indices publish only after a complete successful snapshot.

Source is frozen again. Run formatting, the full serial all-feature workspace
suite, both Clippy feature configurations, and the optimized binary build.

## Final prepublication audit

All 18 original files remain present. Eight exact hashes are unchanged, one
usage page contains its original bytes plus an appended section, and the other
nine source/handoff/index paths are intentionally integrated. Both original
untracked records are byte-identical. The refreshed visible untracked audit
contains 12 accounted-for files, all owned by this standalone boundary.

The final scan covers 198 paths. Its added URL-userinfo check found four
synthetic credential URLs in an unchanged upstream security regression file;
known fixture expectations and upstream identity were checked without
printing their contents. No real credential/provider pattern hits or protected
secret paths were found. The sole large path remains the tracked, smaller
lockfile. All 183 local document links and the whitespace check pass.

## Final release gates

The post-review integrated run passed all five gates: formatting, 243 serial
all-feature Rust tests (zero failed/ignored), default-feature Clippy, all-feature
Clippy (both warnings denied), and the optimized `bu-core` build. The 51 focused
Python merge tests and lock consistency check remain green. No production code
changed after the source freeze; only task records were updated during the run.

Next evidence: intentional merge commit and matching fork remote, then fresh
signed install, installed/build hash match and sequential MCP browser smoke.

## Delivered milestone

Implementation merge `0d9e428e8994510484c553c86f58fba290d72d39` is published on the owner's fork branch
`franky-rust`. The owner clarified that publication is for this fork only.
HTTPS could not request credentials; existing SSH access was verified against
the same owner/repository and the clone's `origin` transport was changed to SSH.
The explicit fork branch push and remote SHA comparison succeeded.

The local executable was rebuilt from that implementation commit, installed
with a fresh inode and valid signature, and matched its signed build artifact
SHA-256 `fcc4da0991569e772b222840968132b2c67770fad94c054da7ef547f8a3c95a0`. Rollback is
`~/.local/bin/browser-use-rs.prev-20261025-182510-570759`. CLI help/version and the
uninitialized EOF exec check pass. The installed MCP lists 21 tools and passed
sequential native date, invalid-date preservation, disabled choice, sensitive
value, checked-state, JPEG capture and owned-browser closure checks. No model
request was sent. Existing MCP clients must reconnect for the new executable.

All 18 original files are accounted for in the merge, including both original
untracked records. The final visible untracked audit is empty; remaining
closure edits are tracked documentation only. The parent workspace remains
untouched, so its prior untracked audit remains current. The final docs commit
and matching remote clean status close the delivery record; no code changes
followed the accepted release gates.

Explicit limits remain in the applicability matrix: Python-only SDK/Bash,
cloud provisioning, agent file-export subsystems and native provider factories
are not claimed as Rust ports. No live provider or other-host deployment was
performed. Module-size follow-up remains a separately bounded issue.

Final closure audit passed: 198 changed paths, 184 resolving local links, zero
visible untracked files, no source changes after release acceptance, and the
installed hash still matches its build artifact. Both remote branch tips were
rechecked against the delivered/frozen source before the docs-only closure.

## Requested build cleanup

The owner requested removal of the Rust build cache after installation. The
cache occupies about 33 GiB (31 GiB debug and 1.6 GiB release). The installed
executable is an independent file, not a symlink into target, and still matches
the recorded install hash. Scope is only `rust/target`; source and rollback
files remain outside the cleanup.

- [x] Clean the explicit Rust target directory with Cargo.
- [x] Confirm the directory is absent and the installed hash, signature and CLI still pass.

Cleanup acceptance: Cargo removed 239564 files / 47.1 GiB logical data; the
directory is absent and observed free space increased by about 32.5 GiB.
The installed hash is unchanged, the signature is valid and CLI startup passes.
No code or dependency changes were needed, and no rebuild was run afterward.
