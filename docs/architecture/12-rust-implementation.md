# 12 · Rust implementation (`browser-use-rs`)

The MCP server deployed on this host is a from-scratch **Rust reimplementation**
of `browser-use --mcp`, living in the Cargo workspace under
[`rust/`](../../rust) on branch `franky-rust`. It reaches **full parity** with the
Python MCP surface — `tools/list` is byte-identical (16 tools, same order, 0
schema diffs) — and is hardened beyond it. This doc is the map from that code to
the design described in docs `00`–`11` (which remain the porting oracle).

The living build log is
[plans/2026-07-05-rust-rewrite/progress.md](../plans/2026-07-05-rust-rewrite/progress.md);
the staged plan is [parity-plan.md](../plans/2026-07-05-rust-rewrite/parity-plan.md).

## Crate map

| Crate | Concern | Design doc |
| --- | --- | --- |
| `bu-core` | The `browser-use-rs` binary (`--mcp` entry). | [07](07-mcp-integration.md) |
| `bu-mcp` | rmcp server: `lib.rs` (16 tool handlers + dispatch), `tools.rs` (tool defs), `tests.rs`. | [04](04-tools-and-action-registry.md), [07](07-mcp-integration.md) |
| `bu-actor` | Single-owner actor serializing all browser work; URL-policy enforcement; per-command timeout. | [00](00-system-overview.md), [02](02-event-bus-and-watchdogs.md), [08](08-actor-scripting-api.md) |
| `bu-cdp` | Chromium via `chromiumoxide`: `lib.rs` (`BrowserSession`/`BrowserPage`), `dom.rs` (three-tree fusion + filtering), `geometry.rs` (`Rect`/`RectUnion`), `security.rs` (`UrlPolicy`), `discovery.rs` (Chromium/env). | [01](01-cdp-transport-and-session-manager.md), [03](03-dom-perception-pipeline.md) |
| `bu-agent` | Perceive→decide→act loop: `action.rs` (multi-action + reasoning schema), `lib.rs` (loop + vision), `report.rs`. | [05](05-agent-control-loop.md) |
| `bu-llm` | Provider-agnostic LLM: `message.rs` (multimodal), `openai.rs` (client + retry), `bedrock.rs` (feature-gated), `LlmProvider` enum. | [06](06-llm-provider-abstraction.md) |
| `bu-dom` | `extract_clean_markdown` for `browser_extract_content`. | [03](03-dom-perception-pipeline.md) |

No production file is oversized; each crate is a single concern and tests live in
sibling `tests.rs` / `live_tests.rs` files.

## Concurrency: the single-owner actor

The Python server relies on an event bus + watchdogs ([02](02-event-bus-and-watchdogs.md)).
The Rust design instead funnels **every** browser operation through one
`BrowserActor` task (`bu-actor`): callers hold an `ActorHandle` and send
`Command`s over an mpsc channel, each carrying a `oneshot` reply. This fixes the
within-process race where rmcp spawns a task per request — no two commands ever
touch Chromium concurrently. Two robustness properties fall out:

- **Stable-`backendNodeId` click cache** — the selector map is keyed by CDP
  backend node id, so a click resolves the same element even if the DOM reorders
  between `get_state` and `click` (no TOCTOU wrong-click).
- **Per-command 90 s timeout** — a wedged renderer (e.g. an `onclick` that spins
  forever) is dropped and the actor survives; a later command still responds.
  Overridable via `BROWSER_USE_COMMAND_TIMEOUT_MS`.

Multi-process isolation is stronger than Python's shared profile: each Chromium
launch gets a unique `user_data_dir`.

## DOM perception ([03](03-dom-perception-pipeline.md))

`bu-cdp/dom.rs` fuses three CDP trees keyed by `backendNodeId`: `DOM.getDocument`
(pierce), `DOMSnapshot.captureSnapshot` (computed styles, paint order, DOM rects),
and `Accessibility.getFullAxTree`. Interactive detection combines a JS-listener
probe (`DOM.resolveNode` → `DOMDebugger.getEventListeners`), AX roles, and
tag/onclick/tabindex heuristics. The element set is then filtered to match
Python's serializer exactly:

- **Paint-order occlusion** (`PaintOrderRemover`): an element fully covered by
  higher-painted **opaque** rects is dropped. Opaque = background ≠
  `rgba(0, 0, 0, 0)` and opacity ≥ 0.8. Uses a disjoint-rect union (`geometry.rs`).
- **Bounding-box containment** (`_apply_bounding_box_filtering`): **tree-based** —
  a propagating interactive ancestor's bounds propagate down the DOM (never across
  siblings), collapsing a ≥99%-contained descendant unless a carve-out applies
  (form control, `label`, nested propagating child, `onclick`, non-empty
  `aria-label`, or `role` in button/link/checkbox/radio/tab/menuitem/option).
  `PROPAGATING_ELEMENTS` is the exact tag+role whitelist (`role=link` excluded).

## Agent loop ([05](05-agent-control-loop.md))

`bu-agent` runs perceive→decide→act. Each step attaches the page **screenshot**
when `use_vision` (default true) via multimodal messages, emits reasoning
(`evaluation_previous_goal`/`memory`/`next_goal`) plus an ordered **multi-action**
list (batch stops after a navigation/click to avoid stale indices), feeds action
errors + extraction results into a persistent read-state, caps consecutive
failures, and synthesizes a best-effort `done` if it runs out of steps. The report
matches Python's `retry_with_browser_use_agent` wording.

## LLM providers ([06](06-llm-provider-abstraction.md))

`bu-llm::LlmProvider` dispatches to an OpenAI-compatible client (`openai.rs`) or,
behind the `bedrock` feature, an AWS Bedrock Converse client (`bedrock.rs`) —
selected by `MODEL_PROVIDER=bedrock`, mirroring Python's `get_default_llm`. The
OpenAI client tolerates `null`/empty `content` (reasoning/refusal responses),
retries 429/5xx + connect/timeout with backoff honoring `Retry-After`, and
defaults model `gpt-4o` + temperature `0.7`. `reqwest`'s default User-Agent avoids
gateway WAF blocks, so no SDK-fingerprint wrapper is needed.

## Security: URL access policy

`bu-cdp/security.rs` ports Python's `SecurityWatchdog` (`_is_url_allowed`,
`_is_url_match`, `_is_ip_address`): allowed/prohibited domains with glob +
root-domain www handling, and `block_ip_addresses` covering dotted/decimal/
hex/octal IPv4 (exotic Unicode forms are canonicalized upstream by the `url`
crate's IDNA). The policy is sourced from `BROWSER_USE_ALLOWED_DOMAINS` /
`BROWSER_USE_PROHIBITED_DOMAINS` / `BROWSER_USE_BLOCK_IP_ADDRESSES`, and
`retry_with_browser_use_agent`'s `allowed_domains` argument scopes an override for
that run (restored after; agent runs serialized behind a mutex so concurrent runs
can't corrupt the base policy).

Enforcement covers **every** navigation and content path, not just the primary
`navigate`: pre-check before navigating, post-redirect reset, new-tab targets,
`guard_active_url` at every observation op (`get_state`/`get_html`/`screenshot`/
`switch_tab`/`close_tab`/`list_sessions`/`page_state`/`go_back`) resetting a
disallowed active page to `about:blank`, and `tabs()` filtering disallowed
background tabs (e.g. from `window.open`). This closed a class of leaks a 3-round
adversarial verification surfaced.

## Hardening provenance

A 10-dimension adversarial audit (each finding cross-examined by 3 lenses)
surfaced 33 confirmed defects; three verification rounds on the fixes found 7
more; a re-run of the initially-failed DOM-filtering dimension found 5 more — all
closed, and the reasoned-only fixes are now covered by executed tests. See
[progress.md](../plans/2026-07-05-rust-rewrite/progress.md).

## Build, test, deploy

```bash
# build the binary (add --features bedrock for AWS)
cargo build --manifest-path rust/Cargo.toml -p bu-core --release

# hermetic tests + lint
cargo test  --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings

# live-Chrome tests (serial — parallel is flaky from concurrent launches, not a bug)
cargo test --manifest-path rust/Cargo.toml --features live-chrome -- --test-threads=1
```

Deployment (install + per-agent registration + env, incl. the required `/v1` base
URL) is in [usage/tools/mcp-multi-agent-setup.md](../usage/tools/mcp-multi-agent-setup.md).
CI lives at [`rust/ci/rust.yml`](../../rust/ci/rust.yml) (the push token lacks
`workflow` scope to place it under `.github/workflows/`).

## Prior art

`browser_use/beta/` drives a separate native Rust core over JSON-RPC; its wire
contract is the conformance oracle, not code owned here — see
[11-beta-rust-bridge.md](11-beta-rust-bridge.md). This workspace is an independent
implementation.
