# Full-parity + concurrency plan (2026-07-05)

Follow-on to [plan.md](plan.md). The Rust MCP server is functionally at 14/14
low-level tools, but a deep review (quality + tool-by-tool parity + DOM gap +
concurrency) found the work to reach **full parity with Python, work better, and
be concurrency-correct**. This is the staged, TDD hand-off for codex. Order is
deliberate: cheap high-leverage correctness first, then the DOM quality lever,
then the LLM capstones.

## Current state (verified)

- `bu-mcp` (~950 LOC) exposes 14/16 tools with byte-identical low-level schemas;
  `bu-cdp` (~820 LOC) is a chromiumoxide wrapper. `bu-dom` is a ~100-line toy
  serializer; `bu-actor`/`bu-bus`/`bu-llm`/`bu-agent`/`bu-tools`/`bu-config`/
  `bu-session` are still stubs.
- **Multi-process concurrency already works** (verified: 4 parallel processes,
  isolated Chrome profiles, correct per-process state) — better than Python's
  shared profile. **Within-process concurrency is broken** (see 1.1).

## Top risks (ranked)

1. **Within-process concurrency race.** rmcp spawns one task per request;
   `shared_page()` clones the page and drops the mutex before browser work → two
   overlapping calls race the shared page, and click/type rebuild the selector
   map and match by *positional* index → TOCTOU wrong-click. Silent, and worsens
   under the exact multi-agent load we target.
2. **Broken advertised contracts:** `browser_click` coordinate mode is advertised
   but rejects coordinate-only calls; `browser_type` never clears the field;
   `include_screenshot` ignored.
3. **DOM quality:** 5-tag heuristic, no JS-listener/AX/visibility/paint-order
   logic, document-space coords dispatched as viewport coords → misses modern
   web, surfaces hidden elements, mis-clicks after scroll.
4. **Missing LLM tools** (`browser_extract_content`, `retry_with_browser_use_agent`).
5. **Robustness:** nanos-only profile dirs, no crash detection, no per-tool
   timeouts, endless CDP-Serde `eprintln` spam, no tracing subscriber.

## Stages (TDD — write the failing test first; mock only the LLM)

### Stage 1 — Concurrency correctness & server hygiene (do first)
- **1.1 Single-owner actor (`bu-actor`).** Move `BrowserSession` + active page into
  one owning task; expose an mpsc `Command` channel (Navigate/GetState/Click/Type/
  …, each with a `oneshot` reply). `BrowserUseMcpServer` holds only the `Sender`.
  Serializes all browser work → kills the race + TOCTOU. *Test:* 8 concurrent
  `get_state` on a fresh server → all Ok, exactly one browser.
- **1.2 backendNodeId selector-map + snapshot cache.** `get_state` caches the map
  keyed by `backend_node_id`; the LLM's integer index → stable `backend_node_id`;
  click/type resolve by cached id (fresh walk only if absent). *Test:* reorder
  DOM after get_state → click(index) still hits the right element or clean
  not-found, never the wrong one.
- **1.3 Type-safe `TabRef` + atomic resolve-and-act** (id-match first; handle
  4-hex suffix collisions; resolve+act under one lock). *Test:* pure unit tests on
  synthetic colliding ids.
- **1.4 Crash-safe profile dir** `browser-use-rs-chromium-{pid}-{uuid7}` + startup
  GC of stale dirs; `remove_dir_all` in `spawn_blocking`. *Test:* 16 threads ×
  1000 iters → all unique+created; SIGKILL → GC sweeps.
- **1.5 Crashed-browser detection/recovery** (`handler.next()==None` → relaunch or
  clean isError). **1.6 Per-tool `tokio::time::timeout`** + robust `go_back`
  (detect no-history/SPA). **1.7 stderr-only `tracing` subscriber** + `if
  matches!(err, CdpError::Serde(_)) { continue }`. **1.8 `bu-core`: serve/usage,
  never a silent no-op.**

### Stage 2 — Tool-contract parity for the 14 shared tools
- **2.1 `browser_click`:** implement coordinate mode + `new_tab`; stop rejecting
  coordinate-only. **2.2 `browser_type`:** clear-first, `text=""` clears, sensitive
  masking. **2.3 `browser_get_state`:** rich Python-shaped payload (JSON in a
  TextContent) with `interactive_elements`/`viewport`/`page`/`scroll`, honor
  `include_screenshot`. **2.4 screenshot viewport dims. 2.5 list_tabs/list_sessions
  shape + real session registry (uuid7). 2.6 Error convention:** recoverable
  failures → `CallToolResult::error` (isError), reserve `ErrorData` for
  INVALID_PARAMS/METHOD_NOT_FOUND. **2.7 `bu-config` parity** (same config file,
  allowed_domains gate). **2.8 return-string golden test.**

### Stage 3 — Full DOM serializer (`bu-dom`): three-tree fusion == Python
- **3.1 Three-tree fusion** (DOM.getDocument + DOMSnapshot.captureSnapshot +
  Accessibility.getFullAXTree) keyed by backendNodeId, ~3 CDP calls not N.
- **3.2 Visibility + DPR/scroll-normalized viewport click coords.**
- **3.3 Interactive detector** (getEventListeners JS-listener probe first, then AX
  roles/props, then heuristics). **3.4 paint-order occlusion culling. 3.5 bbox
  containment filtering. 3.6 serializer polish** (AX-name-first text, scroll info,
  OOPIF recursion with caps, stable element hash, `*` new-element prefix).

### Stage 4 — LLM client + `browser_extract_content` (15/16)
- **4.1 `bu-llm`** OpenAI-compatible reqwest chat client (model/key/base_url from
  config+env; 120s timeout). **4.2 markdown extractor + structure-aware chunker.**
  **4.3 wire `browser_extract_content`** (exact Python schema, `<url>/<query>/<result>`
  framing, extract_links). *Mock the LLM in tests.*

### Stage 5 — Agent loop + `retry_with_browser_use_agent` (16/16, capstone)
- **5.1 bu-llm Bedrock provider. 5.2 `bu-tools` registry** (LLM action → actor
  command). **5.3 `bu-agent` decision loop + AgentHistory. 5.4 wire the tool**
  (exact schema; allowed_domains override only for non-empty list; exact report
  format).

## Definition of full parity (acceptance checklist)

- [ ] `tools/list` = 16 tools in Python order; 14 low-level schemas byte-identical;
      the 2 LLM tools present with exact Python schemas.
- [ ] `get_state`: `content[0].text` is `json.dumps(indent=2)` with url/title/
      tabs/interactive_elements/viewport/page/scroll; `include_screenshot` appends
      an ImageContent + dimensions.
- [ ] `click`: index + coordinate + new_tab modes all work; coordinate-only never
      INVALID_PARAMS; per-mode return strings match Python.
- [ ] `type`: clears by default; `text=""` clears; emails/credentials masked.
- [ ] DOM: JS-listener `div/span` detected; hidden/opacity:0/off-screen/occluded
      absent; nested duplicates collapsed by bbox; OOPIF traversed; index resolves
      via stable backendNodeId; click after scroll lands correctly.
- [ ] Errors: recoverable → isError CallToolResult; ErrorData only for
      INVALID_PARAMS/METHOD_NOT_FOUND.
- [ ] `browser_extract_content` + `retry_with_browser_use_agent` match Python
      output; agent supports OpenAI + Bedrock.
- [ ] Concurrency: N concurrent calls → ≤1 browser, never wrong element, no
      deadlock/starvation, complete under timeout. K processes → isolated
      profiles, zero "profile in use".
- [ ] Hygiene: stdout pure JSON-RPC; no benign-Serde stderr noise at default level;
      `RUST_LOG` enables structured logs; binary serves/usages.

## Works better than Python (bank these wins)

Per-process profile isolation by construction (no shared-profile lock class);
pure-JSON-RPC stdout + RUST_LOG-gated tracing; ~3 CDP round-trips for DOM
(vs N getBoxModel) → faster get_state; dependency-free single-binary startup;
stable-backendNodeId click handle (removes a TOCTOU class); deterministic
single-action ordering via the actor; type-safe tab addressing; crash-safe
profile GC; per-tool timeouts + crashed-browser recovery.

Derived from a 4-analyst review (Rust quality / tool parity / DOM gap /
concurrency) + synthesis; findings are folded into the stages above.
