# DOM Perception: Three-Tree Fusion & Serialization

Layer 5 turns a live Chromium tab into a compact, LLM-legible list of interactive elements. It does this in two halves: `DomService` fires four CDP calls in parallel and **fuses** the DOM tree, the accessibility (AX) tree, and the DOMSnapshot layout tree into one `EnhancedDOMTreeNode` graph keyed by `backendNodeId`; then a 5-stage `DOMTreeSerializer` culls that graph down to the elements worth clicking and emits the `selector_map` whose keys the LLM addresses directly.

Everything here lives in [`browser_use/dom/`](../../browser_use/dom/) and is driven by [`DOMWatchdog`](../../browser_use/browser/watchdogs/dom_watchdog.py) (see [02-event-bus-and-watchdogs.md](02-event-bus-and-watchdogs.md)). The output feeds the tools layer ([04-tools-and-action-registry.md](04-tools-and-action-registry.md)) and the agent loop ([05-agent-control-loop.md](05-agent-control-loop.md)).

## The core contract: `selector_map` index == `backendNodeId`

The single most important invariant in this subsystem. The serializer emits `SerializedDOMState(_root, selector_map)` where `selector_map: dict[int, EnhancedDOMTreeNode]` (`DOMSelectorMap` in [`views.py`](../../browser_use/dom/views.py)). The integer key **is the element's CDP `backendNodeId`**, not a dense 1..N counter:

```python
# serializer.py :: _assign_interactive_indices_and_mark_new_nodes
self._selector_map[node.original_node.backend_node_id] = node.original_node
```

The `[123]<button>` bracket the LLM sees is that backendNodeId. When the model returns `{"click": {"index": 123}}`, resolution is a dict lookup against the cached map — [`BrowserSession.get_dom_element_by_index`](../../browser_use/browser/session.py) just does `self._cached_selector_map[index]`. There is a vestigial `self._interactive_counter` incremented alongside, but it is no longer the key. This means indices are **stable across steps for unchanged elements** (a backendNodeId survives as long as the node does), which is what makes the `is_new` (`*`) marker meaningful.

## Stage 0 — parallel CDP acquisition (`_get_all_trees`)

[`DomService._get_all_trees`](../../browser_use/dom/service.py) gathers the raw material for one target. Four coroutines run under `asyncio.wait(timeout=10.0)` with a one-shot retry of any that time out (`timeout=2.0`), then a hard `TimeoutError` if anything still fails:

| Task | CDP call | Purpose |
|---|---|---|
| `snapshot` | `DOMSnapshot.captureSnapshot(computedStyles=REQUIRED_COMPUTED_STYLES, includePaintOrder=True, includeDOMRects=True)` | layout: bounds, client/scroll rects, paint order, ~9 computed styles |
| `dom_tree` | `DOM.getDocument(depth=-1, pierce=True)` | the structural node tree (pierces shadow roots + same-proc iframes) |
| `ax_tree` | `_get_ax_tree_for_all_frames` → `Accessibility.getFullAXTree(frameId=...)` per frame | roles, accessible names, AX properties |
| `device_pixel_ratio` | `Page.getLayoutMetrics` | CSS-vs-device-pixel ratio |

Two **pre-passes** run before those, sequentially (their cost is logged in `cdp_timing`):

- **iframe scroll capture** — a `Runtime.evaluate` walks `document.querySelectorAll('iframe')` and reads `scrollTop/scrollLeft` of each accessible content doc (cross-origin ones throw and are skipped).
- **JS click-listener detection** — a `Runtime.evaluate` with `includeCommandLineAPI=True` calls the DevTools-only `getEventListeners(el)` over every element, keeping those with `click/mousedown/mouseup/pointerdown/pointerup`. Returned **by reference** (`returnByValue=False`); each object is resolved to a `backendNodeId` via parallel `DOM.describeNode`. This catches framework handlers (Vue `@click`, React `onClick`) on non-semantic tags. It **bails to `null` on pages >10 000 elements** — the loop plus per-element describeNode is O(n) CDP round-trips and blows past 10s. Result: `TargetAllTrees.js_click_listener_backend_ids: set[int]`.

`getFullAXTree` is fetched **per frame** and merged: `_get_ax_tree_for_all_frames` walks `Page.getFrameTree`, requests each frame's AX nodes with `return_exceptions=True`, and concatenates `nodes`. A detaching child frame (ad iframe) is logged and dropped; **the root frame failing re-raises** so the caller's empty-DOM retry path runs instead of silently serving a tree with no main-document AX data.

Gotcha: `snapshot['documents']` is truncated to `max_iframes` (default 100) to prevent iframe-explosion crashes on ad-heavy pages.

## Stage 1 — snapshot flattening (`build_snapshot_lookup`)

[`enhanced_snapshot.py`](../../browser_use/dom/enhanced_snapshot.py) converts the columnar, string-interned DOMSnapshot into `dict[backendNodeId → EnhancedSnapshotNode]`. DOMSnapshot is a struct-of-arrays: node fields index into a shared `strings` table, and layout data is a *separate* parallel array joined by `layout['nodeIndex']`. Two micro-optimizations matter because this is the hot path:

- `layout_index_map = {node_index → layout_idx}` (first occurrence wins) replaces an O(n²) reverse scan.
- `is_clickable_set = set(nodes['isClickable']['index'])` — the raw CDP "rare boolean" is a `list[int]`; membership testing it per-node was the documented #1 bottleneck (**5 925 ms → 2 ms at 20k nodes**).

**Coordinate math lives here.** CDP layout bounds are in **device pixels**; every rect is divided by `device_pixel_ratio` to get CSS pixels (what JS `getBoundingClientRect` sees), fixing coordinate drift on HiDPI/Retina. `EnhancedSnapshotNode` distinguishes three rects with different origins:

- `bounds` — **document** coordinates (page origin, scroll-independent).
- `clientRects` — **viewport** coordinates (scrollport origin).
- `scrollRects` — the element's scrollable content extent (drives `is_actually_scrollable` / `scroll_info`).

Only `REQUIRED_COMPUTED_STYLES` (9 properties: display, visibility, opacity, overflow{,-x,-y}, cursor, pointer-events, position, background-color) are requested — asking for the full computed style set crashes Chrome on heavy sites.

## Stage 2 — three-tree fusion (`get_dom_tree` / `_construct_enhanced_node`)

[`get_dom_tree`](../../browser_use/dom/service.py) builds two lookups — `ax_tree_lookup: dict[backendDOMNodeId → AXNode]` and the snapshot lookup — then recurses `DOM.getDocument`'s tree with the closure `_construct_enhanced_node`, producing one [`EnhancedDOMTreeNode`](../../browser_use/dom/views.py) per DOM node. Fusion is keyed on `backendNodeId`: for each DOM node it grafts on `ax_node = ax_tree_lookup.get(...)`, `snapshot_node = snapshot_lookup.get(...)`, and `has_js_click_listener = backend_node_id in js_click_listener_backend_ids`. The `nodeId → node` memo (`enhanced_dom_tree_node_lookup`) both dedupes and lets each child wire its `parent_node` (parents are always constructed first).

Key data-flow details:

- **Attribute de-interleaving**: CDP ships `attributes` as a flat `[k0,v0,k1,v1,...]`; fusion pairs them into a dict.
- **Shadow DOM**: `shadowRoots` become `EnhancedDOMTreeNode.shadow_roots` (`DOCUMENT_FRAGMENT_NODE`, carrying `shadow_root_type` open/closed) and are filtered out of `children_nodes` so they aren't double-counted. `children_and_shadow_roots` re-unions them for traversal.
- **Same-process iframes**: `contentDocument` recurses inline and back-links `parent_node`.

### Coordinate accumulation across frames

`_construct_enhanced_node` threads a mutable `total_frame_offset: DOMRect` down the recursion so that a node's `absolute_position` is its snapshot `bounds` plus the summed offset of every containing frame:

- entering an `<iframe>`/`<frame>` with bounds → offset **+= iframe.bounds.{x,y}** (child coords are frame-local).
- entering an `<html>` frame node → offset **−= scrollRects.{x,y}** (undo the frame's own scroll).

`absolute_position = snapshot.bounds + total_frame_offset` is the page-absolute box; it's computed here so downstream consumers never re-derive it.

### Visibility (`is_element_visible_according_to_all_parents`)

A classmethod, run per node against the accumulated `html_frames` list. It is a **CSS gate + geometric viewport test**:

1. CSS reject: `display:none`, `visibility:hidden`, or `opacity<=0` → not visible. No `snapshot_node`/`bounds` → not visible.
2. If `viewport_threshold is None`, stop here (CSS-only visibility).
3. Otherwise walk the frame chain in reverse, re-applying iframe offsets and scroll, and reject if the element's box misses the frame viewport by more than `viewport_threshold` (default **1000 px** beyond each edge — a deliberate slack so just-off-screen elements still register). Elements failing the threshold aren't discarded but recorded per-iframe by `_count_hidden_elements_in_iframes` as `{tag, text, pages}` hints so the serializer can print "N more elements below — scroll to reveal".

### Cross-origin iframe recursion

Same-process iframes arrive inline via `pierce=True`. **OOPIFs** (out-of-process, e.g. cross-origin ad/chat widgets) have `contentDocument == None` and live in a different CDP target. When `cross_origin_iframes=True`, fusion recurses into them by calling `get_dom_tree` again with the child's `targetId`, but only if all guards pass: `iframe_depth < max_iframe_depth` (default 5), the iframe is `is_visible`, and it is ≥50×50 px. The child target is found via `BrowserSession.get_all_frames()` (lazily fetched), matched by `frameId` — with a **fallback to `src`-URL matching** for dynamically injected iframes Chrome hasn't yet registered in the frame tree. The accumulated `total_frame_offset` is passed as `initial_total_frame_offset` so absolute coordinates stay coherent across the target boundary. This is disabled by default (`DomService.__init__` `cross_origin_iframes=False`).

## Stages 3–7 — the `DOMTreeSerializer` (5 passes)

[`serialize_accessible_elements`](../../browser_use/dom/serializer/serializer.py) runs five passes over the fused tree, each timed into `timing_info`. Input: the `EnhancedDOMTreeNode` root. Output: `SerializedDOMState(_root: SimplifiedNode, selector_map)`.

### Pass 1 — simplify (`_create_simplified_tree`)
Recursively projects `EnhancedDOMTreeNode` → `SimplifiedNode` (a lighter node holding `original_node`, `children`, and flags: `should_display`, `is_interactive`, `is_new`, `ignored_by_paint_order`, `excluded_by_parent`, `is_shadow_host`, `is_compound_component`). It drops noise (`DISABLED_ELEMENTS` = script/style/head/meta/link/title, and all `SVG_ELEMENTS` decorative children), honors the escape hatch `data-browser-use-exclude[-{session_id}]="true"`, and keeps a node if it is **visible, scrollable, a shadow host, or has kept children**. Notable overrides that *force* inclusion of otherwise-invisible nodes: elements bearing `aria-*`/`pseudo` validation attributes, `input[type=file]` (Bootstrap hides these at opacity 0), and all shadow-host subtrees. This pass also synthesizes **compound components** (`_add_compound_components`): `<select>`, `<input type=range|number|color|file>`, `<details>`, `<audio>`, `<video>` get virtual child descriptors (e.g. a range slider gets `{role:slider, valuemin, valuemax}`; a select gets option counts + first-4 options + an inferred `format_hint`). Date/time inputs deliberately get *no* compound children (their ISO format is shown via placeholder instead).

### Pass 2 — paint-order cull (`PaintOrderRemover.calculate_paint_order`)
[`paint_order.py`](../../browser_use/dom/serializer/paint_order.py) removes elements painted *underneath* opaque higher layers (e.g. a button behind a modal backdrop). Nodes are grouped by snapshot `paint_order` and processed **top layer first**; a `RectUnionPure` accumulates the covered region as a disjoint set of rectangles. Any node whose bounds are already fully covered is flagged `ignored_by_paint_order`. Translucent layers don't occlude: a rect is only *added* to the union if `background-color != rgba(0,0,0,0)` and `opacity >= 0.8`. Safety cap `_MAX_RECTS = 5000` — beyond it, `contains()` conservatively returns False (nothing hidden) to avoid exponential rect fragmentation on layer-heavy pages.

### Pass 3 — optimize (`_optimize_tree`)
Bottom-up prune of structural filler: a node survives only if it is visible, scrollable, a text node, a file input, or has surviving children. Collapses empty wrapper chains.

### Pass 4 — bbox filter (`_apply_bounding_box_filtering`)
De-duplicates the common "clickable inside clickable" pattern. `PROPAGATING_ELEMENTS` (any `<a>`/`<button>`, plus `div/span/input` with `role=button|combobox`) propagate their bounds to **all** descendants; a child ≥`containment_threshold` (default **0.99**) contained within a propagating ancestor is flagged `excluded_by_parent` so it won't get its own index. Exceptions that survive containment: form controls (`input/select/textarea/label`), other propagating elements, `onclick` handlers, meaningful `aria-label`, or interactive `role`. Text nodes are never excluded.

### Pass 5 — index assignment (`_assign_interactive_indices_and_mark_new_nodes`)
The pass that populates `selector_map`. For each non-excluded, non-paint-culled node it calls the cached `ClickableElementDetector.is_interactive` and assigns an index (`= backend_node_id`) when:
- **interactive AND visible**, OR
- an `input[type=file]` (functional despite opacity 0), OR
- a shadow-DOM `input/button/select/textarea/a` that has **no snapshot layout data** (CDP DOMSnapshot often omits shadow content but the element is still real), OR
- a **scrollable container** — always indexed if it looks like a dropdown (`role=listbox|menu|combobox|...`, `<select>`, or `dropdown`-ish class), otherwise only if it has no interactive descendants (avoids indexing scroll wrappers around real buttons).

`is_new` is set when a node's `backendNodeId` was absent from `previous_cached_state.selector_map` (or it's a fresh compound component), and renders as the `*` prefix.

### Interactivity oracle (`ClickableElementDetector.is_interactive`)
[`clickable_elements.py`](../../browser_use/dom/serializer/clickable_elements.py) is a layered heuristic, short-circuiting on the first hit: JS click listener → large iframe (>100px) → search-affordance class/id/data-* → AX properties (`focusable/editable/checked/expanded/...`; `disabled`/`hidden` short-circuit to False) → semantic tags (`button/input/a/select/...`) → interactive attributes (`onclick`, `tabindex`, ...) → interactive `role` (HTML then AX) → icon-sized (10–50px) element with interactive attrs → `cursor:pointer` fallback. It explicitly excludes `html`/`body` and `label[for]` (to avoid double-activating the proxied input).

## Text serialization (`serialize_tree`)

`SerializedDOMState.llm_representation(include_attributes)` calls the static `DOMTreeSerializer.serialize_tree`, walking the `SimplifiedNode` tree into the indented string the LLM reads:

```
[123]<button aria-label=Submit />
	Submit
|SHADOW(open)|[124]<input type=text />
|scroll element|<div />  (2.3 pages below)
```

`_build_attributes_string` filters to `DEFAULT_INCLUDE_ATTRIBUTES`, injects HTML5 date/time **format hints** (`format=YYYY-MM-DD`), pulls the *live* field value from the AX tree (`valuetext`/`value` — the DOM attribute lags typed input), collapses attributes that duplicate the role/text, and **hard-blocks `<input type=password>` values** from ever entering the snapshot (prompt-injection exfiltration guard). SVG subtrees render as a single collapsed `<svg ... /> <!-- content collapsed -->`. `eval_representation` is a sibling serializer ([`eval_serializer.py`](../../browser_use/dom/serializer/eval_serializer.py)) for judge/eval contexts that keeps structure but omits indices.

## Element hashing & `MatchLevel` (history replay)

To re-run a recorded action after the DOM has changed, backendNodeId is useless (it's per-load). `EnhancedDOMTreeNode` provides content-addressed hashes over `parent_branch_path (tag names root→node) | sorted static attributes | ax_name`:

- `__hash__` / `element_hash` — SHA-256 over all `STATIC_ATTRIBUTES` (first 16 hex → int).
- `compute_stable_hash` — same, but `class` is passed through `filter_dynamic_classes`, dropping transient state tokens (`focus/hover/active/open/...` in `DYNAMIC_CLASS_PATTERNS`).

At save time, [`DOMInteractedElement.load_from_enhanced_dom_tree`](../../browser_use/dom/views.py) snapshots `element_hash`, `stable_hash`, `xpath`, and `ax_name`. On replay, [`Agent._update_action_indices`](../../browser_use/agent/service.py) cascades the [`MatchLevel`](../../browser_use/dom/views.py) enum against the *current* selector_map, taking the first hit: **EXACT** (element_hash) → **STABLE** (stable_hash) → **XPATH** (structural path, breaks at shadow/iframe boundaries) → **AX_NAME** (node name + accessible name; robust to SPA re-renders) → **ATTRIBUTE** (unique `name`/`id`/`aria-label`, for legacy history without `stable_hash`). No match → the action is dropped. A `TODO` in `__hash__` notes the intent to migrate to `backendNodeId + sessionId`, which the live path already uses.

## Invariants & gotchas

- **`selector_map` key ≡ `backendNodeId`.** Indices are stable across steps for surviving nodes; that's what makes `*`-new markers work.
- Fusion joins on `backendNodeId`; AX/snapshot data is *optional* per node (either lookup can miss — shadow-DOM inputs routinely lack snapshot data yet are still indexed).
- `bounds` (document) ≠ `clientRects` (viewport) ≠ `scrollRects` (content). Mixing them silently breaks visibility/scroll math.
- All layout rects are divided by `device_pixel_ratio` **once**, in `build_snapshot_lookup`. Don't re-scale downstream.
- Two silent degradations under load: JS-listener detection off above 10k elements; paint-order union caps at 5000 rects. Both fail *open* (fewer culls, never wrong clicks).
- Cross-origin iframe descent is **off by default** and gated on depth/visibility/size.
- A fresh websocket-derived tree is built per step (`DomService.__init__` docstring flags making CDP sessions persistent as a TODO).

## Rust port notes

- **Maps cleanly.** The whole pipeline is pure data transformation over CDP JSON — ideal for Rust. `EnhancedDOMTreeNode`/`SimplifiedNode` → `struct`s in an arena/`Vec` (use `id_arena` or index-based `Vec<Node>` instead of the Python `parent_node` back-pointers, which fight the borrow checker). CDP types come typed from a `cdp`-equivalent crate. Hashing is already `sha256` (use `sha2`). `serde_json` for the columnar DOMSnapshot decode.
- **`RectUnionPure`** is a self-contained computational-geometry kernel (disjoint-rect subtraction); trivially portable and a candidate for SIMD/`geo`-crate acceleration. Watch the `_MAX_RECTS` fail-open behavior — keep it.
- **Parallel CDP acquisition** — `asyncio.wait` with timeout+retry maps to `tokio::time::timeout` + `futures::join!`/`JoinSet`. Per-frame AX gather → `JoinSet` with per-task error tolerance.
- **Hard parts:** (1) the mutable `total_frame_offset` threaded through recursion needs care as an explicit accumulator param (no shared-mutable `DOMRect`); (2) the DevTools-only `getEventListeners` command-line API has no CDP-native equivalent — the Rust core must reproduce the `Runtime.evaluate(includeCommandLineAPI=true)` + by-reference `describeNode` dance exactly. The `browser_use/beta/` Rust bridge ([11-beta-rust-bridge.md](11-beta-rust-bridge.md)) is the authoritative contract for how much of this a native core already owns.
- The visibility/coordinate heuristics are **empirically tuned** (1000 px threshold, 0.8 opacity, 0.99 containment, 50 px iframe floor). Port them as named constants, not inlined magic — they encode hard-won web-compat lessons and will need the same tuning knobs.

---

See also: [index.md](index.md) · [00-system-overview.md](00-system-overview.md) · [02-event-bus-and-watchdogs.md](02-event-bus-and-watchdogs.md) · [04-tools-and-action-registry.md](04-tools-and-action-registry.md) · [05-agent-control-loop.md](05-agent-control-loop.md)
