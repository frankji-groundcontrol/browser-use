# Whole-repository review findings

Date: 2026-09-16. Review commit: `823f8782fa97f8310c4158e87d668d90b9355672` on
`franky-rust`. The initial review was read-only; the resolution update below records the subsequent hardening changes and proof.

## Confirmed or high-confidence findings

| ID | Severity | Location | Trigger and observed behavior | Expected behavior / remediation | Confidence |
| --- | --- | --- | --- | --- | --- |
| SEC-01 | High | `browser_use/browser/watchdogs/security_watchdog.py:252-287`; `rust/crates/bu-cdp/src/security.rs:97-125` | A full URL allowlist such as `https://example.com` is matched with raw `startswith`. The sentinel URLs `https://example.com.evil.test/path` and `https://example.com@evil.test/path` both returned `True` in the Python helper. Rust has the same operation. | Parse and compare scheme, host boundary, port and path explicitly, or document and reject full URL patterns that cannot be safely bounded. Add suffix-host and userinfo regressions. | High; deterministic Python proof and matching Rust code. |
| SEC-02 | High | `rust/crates/bu-actor/src/lib.rs:492-510, 671-720` | Indexed and coordinate click/type/select/scroll commands can act on the current page without calling `guard_active_url`. A page can navigate by script after the last observation, then receive an action on a prohibited origin. | Apply one policy guard immediately before every mutating action, including coordinates and evaluate-like paths. Add a live redirect-then-click test. | High static; live reproducer remains a follow-up. |
| SEC-03 | High | `browser_use/config.py:324-357` | Malformed or temporarily unreadable `config.json` is caught by a broad exception and overwritten with fresh defaults. A temporary `{bad` file was rewritten (`config_rewritten True`). New files use ordinary mode (`0o644` in the repro) and the default model entry includes an API-key placeholder. | Preserve the original file, fail with an actionable recovery path, and write credential-bearing configuration with restrictive permissions. | High deterministic repro. |
| SEC-04 | High privacy | `browser_use/config.py:201`; `browser_use/agent/service.py:2183-2246`; `browser_use/telemetry/views.py:10-21`; `browser_use/telemetry/service.py:79-121` | Telemetry is enabled by default and serializes the task, action history, visited URLs, final result, errors and judge reasoning unchanged into event properties. A sentinel capture reached the properties unchanged. | Make collection opt-in or clearly disclose raw fields; redact credentials/PII and URLs before serialization; provide field-level controls. | High static plus sentinel proof; actual provider transmission was not invoked. |
| SEC-05 | High conditional privacy | `browser_use/skills/service.py:194-221`; `browser_use/browser/session.py:1417-1421`; `browser_use/agent/service.py:934-954` | Cookie injection builds a name-only map from all browser cookies and ignores a skill parameter's `cookie_domain`. Same-name cookies from two domains resolve last-wins, so a remote skill requesting one domain can receive another domain's cookie when both exist. | Filter by exact domain/path/secure context before injecting; never expose all cookies to a remote skill. | High conditional on a selected remote skill and colliding cookies; deterministic map proof. |
| REL-01 | High | `bin/setup.sh:18-19,36-43` | A clean checkout runs `uv venv` from `bin/`, while `uv sync` discovers the ancestor project and creates the root environment. The final `uv pip show browser-use` resolves the nearer empty `bin/.venv` and exits non-zero. | Run setup from the repository root and verify the same environment explicitly (or use `uv run`). | High; reproduced safely in a temporary checkout. |
| REL-02 | High | `.github/workflows/publish.yml:34-74,76-118,137`; `pyproject.toml:5` | Tag creation and publishing are independent jobs. A manual dispatch can publish the dispatched checkout while the tag job computes a new RC tag; package metadata remains the static version, so a new tag can carry an old or colliding artifact. | Make publish depend on tag creation, pass the exact ref/version, and assert artifact version equals the tag before upload. | High static and version arithmetic proof. |
| REL-03 | Medium/High | `.gitignore:50`; `Dockerfile.fast:4,21-26` | `uv.lock` is ignored and absent from a checkout, while the fast image uses `uv sync --locked` against a lock inherited from a prebuilt base image. A dependency/metadata change after that base is built makes the fast build fail. | Track and refresh the lock used by the fast image, or generate it in the same build and pin the base/image pair. | High conditional on stale base image; standard Dockerfile generates its lock and is not affected. |
| RST-01 | High | `rust/crates/bu-actor/src/lib.rs:492-510, 671-720` | Rust mutating actions omit the URL guard described by the security contract. This is the Rust counterpart of SEC-02. | Centralize pre-action policy enforcement and add a regression. | High static. |
| RST-02 | Medium/High | `rust/crates/bu-cdp/src/lib.rs:353-359,382-404` | Attach connects to an existing browser and selects from `pages()` without an explicit existing-target fetch. Chromiumoxide documents that existing targets require `fetch_targets`; the current live probe only covered a newly created page. | Fetch and select existing targets deterministically; add a pre-existing titled-tab attach test. | Medium: library contract and code path; needs pinned-Chromium confirmation. |
| RST-03 | Major gap | `rust/crates/bu-bus/src/lib.rs`, `bu-config/src/lib.rs`, `bu-session/src/lib.rs`, `bu-tools/src/lib.rs` | These crates are literal placeholder libraries while architecture docs claim a broad/full Rust port. Cargo can pass without proving those contracts. | Either implement the declared boundaries or change the parity claim and track the missing work. | High source inspection. |
| RST-04 | Major gap | Rust LLM/provider modules and feature gates | No evidence of streaming/SSE, provider tool-call serialization, or usage accounting parity; Bedrock is feature-gated and not covered by default tests. | Add provider wire-contract fixtures and feature-matrix tests for streaming, tools, retries, timeouts and usage. | High gap; real provider credentials unavailable. |
| RST-05 | Medium | `rust/crates/bu-cdp/src/lib.rs:54-73,353-357` | HTTP-origin and raw WebSocket attachment both worked against the isolated Chrome probe. A `/json/version`-suffixed URL also happened to work on this Chromium, but the Rust helper appends the suffix unconditionally and includes the full capability URL in attach errors. | Match Python's suffix handling and redact path/query from errors. | Medium parity/privacy concern; documented origin path is proven. |
| TEST-01 | High test-quality | Rust all-feature tests | `cargo test --workspace` passed its default tests. `cargo test --workspace --all-features -- --test-threads=1` failed in `bu-actor::a_capture_that_never_completes_leaves_no_stale_indices` because the browser actor channel closed; focused/serial runs have also varied by host. | Make live-browser tests provision/synchronize their browser and report environmental skips separately; require a stable feature matrix in CI. | High for “all features are green” being unproven; product defect not established. |

## Correctness and reliability candidates

The following are supported by source tracing and should be fixed or covered by
targeted tests, but are lower confidence until a live reproducer is added:

- `browser_use/actor/element.py:247-264` computes and clamps a click point before
  scrolling, then uses the stale point; recompute after scroll.
- `browser_use/actor/element.py:624-638` sends a drag movement without the CDP
  button bitmask, so native drag targets may not receive a drag gesture.
- `browser_use/actor/page.py:53-70` has an unsynchronized attach check; concurrent
  callers can create duplicate sessions.
- `browser_use/browser/watchdogs/local_browser_watchdog.py:146-180` can leave a
  launched child alive when CDP readiness fails, and pipes are not drained.
- `browser_use/browser/watchdogs/local_browser_watchdog.py:85-90` kills a local
  subprocess without consulting `keep_alive`.
- `browser_use/browser/watchdogs/crash_watchdog.py:75-78` asserts the agent focus
  target instead of using the event target.
- `browser_use/agent/service.py:1706-1717,2275-2281,2494-2498` detects only
  coroutine functions, so async callable objects can return un-awaited coroutines.
- `browser_use/beta/service.py:5215-5218,6298-6303` accepts timeout settings but
  discards them and can block on close RPCs.
- `browser_use/dom/service.py:357-386` fans out through frames before applying
  useful limits.

## Explicitly rejected or blocked claims

- Missing `/VERSION.txt` is not a Docker build failure: command substitution in
  `echo "$(cat /VERSION.txt)"` is masked by the successful `echo` status.
- The standard Dockerfile is not proven broken by an absent lock: it generates an
  unlocked lock before its later locked sync. Only the fast-image stale-base case
  is retained.
- Broad PR-token/secrets language is hardening guidance, not a demonstrated fork
  exploit under GitHub's `pull_request` protections.
- Direct external-file reads, sandbox cloudpickle execution, and raw Gmail bodies
  are intentional trust-boundary APIs; no bypass was proven in this review.
- Docker daemon execution, real external LLM credentials, and a beta child-process
  death matrix were blocked by the local environment/credential boundary. They are
  recorded as unverified, not passed.

## Verification record

- Python `tests/ci`: **1075 passed, 34 skipped in 917.14s**.
- Python Pyright: 0 errors; its configured exclusions leave beta service and some
  provider modules outside the proof. Ruff `--no-fix` reports one import-order
  error in `browser_use/browser/profile.py`; format check reports two files.
- Python wheel and sdist: built successfully for version 0.13.8.
- Rust default workspace tests: passed; `cargo fmt --all -- --check` and all-target
  all-feature Clippy with `-D warnings`: passed.
- Rust all-feature tests: failed as described in TEST-01.
- Rust MCP live probe against an isolated Chrome: HTTP and raw WebSocket attach,
  initialize, tools/list (19), navigate, state, screenshot, unknown-tool error,
  and external-browser survival all passed. No raw endpoint or credential was
  recorded.
- Documentation local-link audit: 0 missing links. `scripts/check-docs-recorded.sh`
  passed. `uv lock --check` passed with a deprecation warning.

## Area disposition matrix

| Required area | Disposition | Evidence boundary |
| --- | --- | --- |
| Public API and configuration | Partial: imports/type/build pass; config overwrite and permission defect confirmed | CLI/provider credential permutations remain unproven |
| Browser and CDP lifecycle | Partial: HTTP/WS attach and external ownership pass | Existing-target discovery, crash/reconnect/cleanup edge cases need live fixtures |
| Security boundaries | Findings confirmed for URL policy, telemetry and cookie scoping | Docker/provider endpoint and hostile redirect matrix blocked |
| DOM and interaction correctness | Partial: Python CI and focused iframe tests pass; interaction candidates remain | Nested zero-scroll and drag/scroll edge cases need dedicated live tests |
| Agent behavior | Partial: CI behavior matrix passes | Real model retries/cancellation and beta process death blocked |
| LLM providers | Partial: static serialization/unit coverage | Real provider credentials, streaming and Bedrock matrix unavailable |
| MCP contracts | Proven for local Rust stdio handshake, 19 tools, errors and screenshots | Full Python/Rust schema golden diff remains a follow-up |
| Beta Rust bridge | Blocked for child-process death, malformed frames and truncation | No controlled beta child harness available in this run |
| Rust implementation | Partial: default tests, format and Clippy pass; parity/placeholders and action guard gap confirmed | Feature live tests fail before browser actor startup |
| Operational reliability | Partial: package build, docs links and scripts checks pass; setup/publish/fast-image defects confirmed | Docker daemon and host install matrix unavailable |
| Test quality | Proven gap: 1075/34 Python result and failing Rust all-feature gate recorded | Coverage cannot establish behavior for blocked external systems |

This matrix is the completion check: every requested area is marked proven,
partial with a concrete gap, or blocked with the reason; no blocked gate is
represented as a pass.

## Resolution update (2026-09-16)

The hardening plan in
[`docs/plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md`](../plans/2026-09-16-hardening-review-findings/2026-09-16-hardening-review-findings.md)
implemented every retained finding. The original observations above remain as
historical evidence; this table records the final disposition and reproducible
proof.

| ID | Final disposition | Proof |
| --- | --- | --- |
| SEC-01 | Fixed and proven | Python `tests/ci/security/test_data_boundaries.py::test_full_url_allowlist_requires_exact_authority`; Rust CDP security URL boundary tests. Scheme, host, userinfo, port, and path boundaries are parsed explicitly. |
| SEC-02 / RST-01 | Fixed and proven | Rust actor guards every mutating action immediately before dispatch; MCP live policy/action tests pass in the all-feature matrix. |
| SEC-03 | Fixed and proven | Malformed config preservation and owner-only file mode tests pass; writes use mode `0600` and never replace unreadable input. |
| SEC-04 | Fixed and proven | Telemetry is opt-in, metrics-only by default, and disables task/action/error/URL/geo-IP/autocapture fields; focused data-boundary tests pass. |
| SEC-05 | Fixed and proven | Cookie injection requires declared domain scope and enforces domain/path boundaries; focused scope regression passes. |
| REL-01 | Fixed and proven | Setup script resolves the repository root, creates and verifies one environment, and shell syntax checks pass. |
| REL-02 | Fixed and proven | Release contract tests pass; publish waits for the exact tag and validates source, wheel, and sdist versions before upload. |
| REL-03 | Fixed and proven | `uv.lock` is tracked and Docker fast/base builds use the pinned lock with `--locked`; lock consistency and static Docker checks pass. Docker daemon execution was unavailable locally. |
| RST-02 | Fixed and proven | Attach starts the handler before `fetch_targets`; `bu-cdp` live test attaches to a pre-existing browser and preserves external ownership. |
| RST-03 | Fixed and proven | Architecture and contracts now state the implemented Rust boundary and explicitly reserve empty crates; no full-parity claim remains. |
| RST-04 | Fixed and proven | Typed completion/stream APIs cover OpenAI Responses/Chat and Anthropic tool calls, retries, timeouts, truncation, and usage fixtures. Bedrock Converse remains text/image-only and returns a tested explicit unsupported error for typed tools/streaming. `bu-llm --all-features`: 61 passed. |
| RST-05 | Fixed and proven | DevTools resolver accepts raw HTTP/WS endpoints, avoids duplicate `/json/version`, preserves query handling, bounds requests, and redacts capability URLs from errors. |
| TEST-01 | Fixed and proven | Env-isolated `cargo test --workspace --all-features -- --test-threads=1`: all workspace unit/doc tests passed, including 21 CDP, 37 MCP, 61 LLM, and 9 agent tests. |
| Candidate lifecycle/DOM items | Fixed and proven | Focused Python actor/lifecycle tests cover stale geometry, drag buttons, concurrent attach, keep-alive ownership, subprocess pipes/reaping, crash target selection, async callbacks, beta timeout/cleanup/cancellation, and bounded frame traversal. Rust close/reap is bounded and MCP live tests pass. |

External-provider credentials, Docker daemon execution, and a controlled beta
child-process death harness remain unavailable in this environment. Their
wire-contract, static, and isolated-fixture checks pass; those unavailable
external runs are recorded as blocked rather than claimed as live proof.
