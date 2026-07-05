# Actor: Imperative CDP Scripting Facade

The `actor` package is a Playwright-shaped, imperative wrapper over raw CDP that lets you drive a tab (or iframe) directly — `page.click`, `element.fill`, `mouse.scroll` — **without** touching the [event bus](02-event-bus-and-watchdogs.md) or the [Agent](05-agent-control-loop.md). It is keyed entirely on CDP identifiers (`targetId`, `sessionId`, `backendNodeId`), talks straight to the root `CDPClient`, and bridges back into the LLM stack only through two optional AI helpers.

See the operator example in [`examples/custom-functions/actor_use.py`](../../examples/custom-functions/actor_use.py). Related layers: [CDP Transport & Session Management](01-cdp-transport-and-session-manager.md), [DOM Perception](03-dom-perception-pipeline.md), [LLM Provider Abstraction](06-llm-provider-abstraction.md); index at [index.md](index.md).

## File map

| File | Role |
| --- | --- |
| [`browser_use/actor/page.py`](../../browser_use/actor/page.py) | `Page` — tab/iframe-level ops keyed by `targetId`; navigation, `evaluate`, screenshots, keyboard, AI helpers (564 lines). |
| [`browser_use/actor/element.py`](../../browser_use/actor/element.py) | `Element` — node-level ops keyed by `backendNodeId`; the click/fill fallback ladders (1182 lines). |
| [`browser_use/actor/mouse.py`](../../browser_use/actor/mouse.py) | `Mouse` — raw coordinate mouse + scroll ladder. |
| [`browser_use/actor/utils.py`](../../browser_use/actor/utils.py) | `Utils.get_key_info` — key-name → `(code, windowsVirtualKeyCode)` table. |
| [`browser_use/actor/__init__.py`](../../browser_use/actor/__init__.py) | Exports `Page`, `Element`, `Mouse`, `Utils`. |

The facade is minted from [`BrowserSession`](../../browser_use/browser/session.py): `new_page()`, `get_current_page()` / `must_get_current_page()`, `get_pages()`, `close_page()` (session.py lines 1312-1391). Each constructs `Page(self, target_id)` — the `BrowserSession` is passed in only to reach `browser_session.cdp_client`.

---

## Object model: three handles keyed on CDP ids

The three classes are thin, stateful handles — each holds a `BrowserSession`, a cached `self._client = browser_session.cdp_client`, and one or two CDP identifiers.

- **`Page(browser_session, target_id, session_id=None, llm=None)`** — one CDP *target* (a tab or an OOPIF). Lazily attaches to get a `sessionId`.
- **`Element(browser_session, backend_node_id, session_id=None)`** — one DOM node, identified by its **`backendNodeId`** (the stable, session-independent id from the DOM/AX/layout fusion; see [DOM Perception](03-dom-perception-pipeline.md)). Note the contract from the serializer: the interactive `selector_map` "index" *is* the `backendNodeId`, which is exactly what `Page.get_element(backend_node_id)` consumes.
- **`Mouse(browser_session, session_id, target_id)`** — coordinate-space input, lazily created via `await page.mouse`.

`Element` never stores a `nodeId`; it resolves one on demand because `nodeId`s are ephemeral per `DOM.getDocument` generation:

```python
async def _get_node_id(self) -> int:                     # backendNodeId → nodeId
    params = {'backendNodeIds': [self._backend_node_id]}
    result = await self._client.send.DOM.pushNodesByBackendIdsToFrontend(params, session_id=self._session_id)
    return result['nodeIds'][0]

async def _get_remote_object_id(self) -> str | None:      # → JS RemoteObject.objectId
    node_id = await self._get_node_id()
    result = await self._client.send.DOM.resolveNode({'nodeId': node_id}, session_id=self._session_id)
    return result['object'].get('objectId', None)
```

This resolve-per-call is why a page mutation between two `Element` calls can surface as `"Failed to find DOM element based on backendNodeId, maybe page content changed?"`.

### Lazy session attach

`Page` does not attach until an op needs a `sessionId`. `_ensure_session()` is the gate:

```python
async def _ensure_session(self) -> str:
    if not self._session_id:
        result = await self._client.send.Target.attachToTarget({'targetId': self._target_id, 'flatten': True})
        self._session_id = result['sessionId']
        await asyncio.gather(
            self._client.send.Page.enable(session_id=self._session_id),
            self._client.send.DOM.enable(session_id=self._session_id),
            self._client.send.Runtime.enable(session_id=self._session_id),
            self._client.send.Network.enable(session_id=self._session_id),
        )
    return self._session_id
```

`flatten=True` mirrors the transport-layer `setAutoAttach(flatten)` contract — all sessions ride the one multiplexed WebSocket keyed by `sessionId`. The public `async property session_id` just awaits `_ensure_session()` so you can pass the id into an arbitrary CDP call yourself.

---

## The deliberate bus-bypass

Every actor call reaches for `browser_session.cdp_client` — the **root** `CDPClient` (session.py line 1307), the same handle the watchdogs use, but invoked *directly*. There is no `event_bus.dispatch(...)`, no `ClickElementEvent`, no watchdog RPC. Consequences, by design:

- **No watchdog side-effects.** Downloads, popups, security-domain checks, DOM-cache invalidation, screenshot capture — none of the bus-mediated services in [layer 4](02-event-bus-and-watchdogs.md) observe an actor click. The actor trades the Agent's safety net for latency and control.
- **No `_cached_selector_map` coupling.** The Agent path keys `index → EnhancedDOMTreeNode` through the session's cached selector map; the actor path re-derives geometry from CDP each call.
- **Concurrency is your problem.** Actor calls do not serialize against the Agent's step loop; interleaving actor mutations with a running Agent is unsupported.

This is the intended contract: the actor is the escape hatch for imperative scripting and for custom `@tools.registry.action` functions (see the example) that want fine control inside a single step.

---

## The fallback-ladder interaction pattern

The signature actor idiom is a **degradation ladder**: try the most faithful CDP mechanism, silently catch, fall to a coarser one, and end at a JavaScript brute-force. `Element.click()` ([element.py](../../browser_use/actor/element.py) lines 93-351) is the canonical example — geometry acquisition descends four rungs:

1. **`DOM.getContentQuads(backendNodeId)`** — best for inline/wrapped elements and multi-rect layouts; yields quad list.
2. **`DOM.getBoxModel(backendNodeId)`** → take `model.content` (8 floats), reshape into one quad.
3. **`Runtime.resolveNode` + `callFunctionOn` `getBoundingClientRect()`** — JS rect, reshaped into a quad.
4. **JS `this.click()`** — if no geometry at all, resolve the object and call `.click()`, `sleep(0.05)`, return.

With quads in hand it computes the **largest viewport-visible quad** (intersecting each quad against `Page.getLayoutMetrics().layoutViewport`), takes that quad's centroid, clamps it inside the viewport, `DOM.scrollIntoViewIfNeeded`, then synthesizes a real pointer sequence via `Input.dispatchMouseEvent`: `mouseMoved` → `mousePressed` → `mouseReleased`, carrying a modifier bitmask (`Alt=1, Control=2, Meta=4, Shift=8`). The press/release are each wrapped in `asyncio.wait_for` (1 s / 3 s) and swallow `TimeoutError`. If the whole synthetic-input block throws, it drops to the same JS `this.click()` brute-force — a *fifth* rung.

`fill()` (lines 353-507) runs two nested ladders of its own:

- **Focus ladder** `_focus_element_simple`: `DOM.focus(backendNodeId)` → JS `this.focus()` → synthetic click at cached coordinates.
- **Clear ladder** `_clear_text_field`: JS `this.select(); this.value=""` + dispatch `input`/`change` events (React-friendly), *verify* the value emptied, else triple-click + `Delete`.

Then it types **character-by-character**, emitting the proper `keyDown` (no `text`) / `char` (with `text`) / `keyUp` triple per char, with an 18 ms inter-keystroke delay and `\n` mapped to a full Enter sequence. `Mouse.scroll()` ([mouse.py](../../browser_use/actor/mouse.py) lines 85-134) is the same shape: `Input.dispatchMouseEvent(mouseWheel)` → `Input.synthesizeScrollGesture` → JS `window.scrollBy`.

Other `Element` ops are single-rung: `hover`/`drag_to` dispatch mouse events off `get_bounding_box()`; `select_option` walks `DOM.requestChildNodes`/`describeNode` and clicks the matching `<option>`; `check` just delegates to `click`. `get_bounding_box()` derives an axis-aligned box from the `getBoxModel` content quad (min/max over the 4 corners) and returns `None` on any failure.

---

## `evaluate`: arrow-function string surgery

Both `Page.evaluate` and `Element.evaluate` take a **string** that must be an arrow function, and both stringify the return (`''` for `None`/undefined, `json.dumps` for dict/list, `str()` otherwise). But they compile it differently because they hit different CDP endpoints.

**`Page.evaluate(page_function, *args)`** ([page.py](../../browser_use/actor/page.py) lines 103-190) targets `Runtime.evaluate`, which wants an *expression*, so it wraps the arrow in an IIFE:

```python
page_function = self._fix_javascript_string(page_function)          # strip Python-string artifacts
if not (page_function.startswith('(') and '=>' in page_function):
    raise ValueError('JavaScript code must start with (...args) => format')
expression = f'({page_function})({", ".join(json.dumps(a) for a in args)})'   # args JSON-encoded inline
```

`_fix_javascript_string` is heuristic cleanup for JS that arrived as a Python string literal: it strips outer wrapper quotes and un-escapes `\"`/`\'` only when the escaped count exceeds the unescaped count. It's conservative but genuinely a guess — a gotcha if your JS legitimately contains more escaped than unescaped quotes.

**`Element.evaluate(page_function, *args)`** (lines 711-829) targets `Runtime.callFunctionOn` with the element as `this`, which wants a *function declaration*, not an arrow. So it regex-transpiles arrow → `function`:

```python
is_async = page_function.strip().startswith('async')
arrow_match = re.match(r'\s*\(([^)]*)\)\s*=>\s*(.+)', func_to_parse, re.DOTALL)
params_str, body = arrow_match.group(1).strip(), arrow_match.group(2).strip()
# expression body → wrap in `return`; block body ({...}) → use as-is
decl = f'{async}function({params_str}) {{ return {body}; }}'   # or `... {body}` if body startswith '{'
```

Args become `CallArgument(value=arg)` objects instead of inline JSON. Both raise `RuntimeError` on `exceptionDetails`. The regex is the fragile part: a `)` inside a default parameter value would break the `([^)]*)` capture.

---

## Keyboard synthesis

Actor input builds its own US-keyboard model rather than leaning on CDP's `insertText`:

- **`Page.press(key)`** parses `"Control+A"`-style combos, computes the modifier bitmask, presses modifiers, presses/releases the main key with modifiers, then releases modifiers in reverse — each via `Input.dispatchKeyEvent` with `code`/`windowsVirtualKeyCode` from `get_key_info`.
- **`Utils.get_key_info(key)`** ([utils.py](../../browser_use/actor/utils.py)) is a ~130-entry table mapping key names to `(code, windowsVirtualKeyCode)` (nav, modifiers, F1–F24, numpad, OEM punctuation, media), with dynamic fallbacks for single alnum chars. Exposed both as a static method and a module-level function.
- **`Element._get_char_modifiers_and_vk` / `_get_key_code_for_char`** encode the shifted-symbol map (`!`→Digit1+Shift, `@`→Digit2+Shift, …) so typed characters produce physically plausible `keyCode`/`code`/modifier triples. There's even a Unicode guard: `'ß'.upper() == 'SS'` would break `ord()`, so it falls back to the original code point.

This much machinery exists because sites gate on realistic `KeyboardEvent.code`/`keyCode`, not just the resulting text.

---

## AI helpers: bridging back into DomService + LLM

Three `Page` methods reach back across the bus-bypass into the perception + LLM layers. They take an optional `llm` (falling back to the `Page`'s constructor `llm`), and raise `ValueError('LLM not provided')` if neither exists.

**`get_element_by_prompt(prompt, llm=None) -> Element | None`** (page.py lines 399-478):

1. `DomService(browser_session).get_dom_tree(target_id=self._target_id, all_frames=None)` — fuse the three CDP trees (lazy cross-origin iframe fetch).
2. `DOMTreeSerializer(tree, None, paint_order_filtering=True, session_id=...).serialize_accessible_elements()` → `serialized_dom_state`.
3. `serialized_dom_state.llm_representation()` becomes the `[index]<type>text</type>` browser-state string in a hand-written system prompt.
4. `llm.ainvoke([system, state], output_format=ElementResponse)` where `ElementResponse` is a locally-declared `BaseModel` with a single `element_highlight_index: int | None`.
5. Look the index up in `serialized_dom_state.selector_map`; return `Element(browser_session, node.backend_node_id, session_id)` or `None`.

`must_get_element_by_prompt` wraps it and raises if `None` — but note the LLM itself can still legitimately answer "no match," so `must_` is about the caller's contract, not model reliability.

**`extract_content(prompt, structured_output: type[T], llm=None) -> T`** (lines 491-554) is the actor twin of the Tools `extract` action:

1. `_extract_clean_markdown()` delegates to the shared [`extract_clean_markdown`](../../browser_use/dom/markdown_extractor.py) via the `dom_service`+`target_id` path (the same helper the Tools service uses via the `browser_session` path — one function, two call shapes, mutually exclusive args).
2. A structured-extraction system prompt + `<query>/<webpage_content>` user message go to `llm.ainvoke(..., output_format=structured_output)` under a 120 s `asyncio.wait_for`.
3. Returns `response.completion` — the populated pydantic instance.

These are the only actor entry points that consume [DomService](03-dom-perception-pipeline.md) and [BaseChatModel](06-llm-provider-abstraction.md); everything else is pure CDP.

---

## Invariants & gotchas

- **`backendNodeId` is the actor's currency.** `Element` is valid only as long as that node exists; there is no auto-refresh. Stale ids surface as "maybe page content changed?" errors.
- **Silent ladders hide failures.** The `except Exception: pass` rungs mean a click can "succeed" via JS `this.click()` even when synthetic input was blocked — behaviorally different (no hover, no focus, no `:active`). Good for robustness, bad for debugging.
- **`evaluate` return values are always strings**, JSON-encoded for containers. Callers must re-parse.
- **`session_id` threading.** `Element`s minted by AI helpers reuse the `Page`'s session id; ones from `get_elements_by_css_selector` carry the freshly-attached id. A `Page` op run before `_ensure_session` (rare) would pass `session_id=None`.
- **No re-entrancy with the Agent.** Actor and Agent both mutate the real page but neither locks the other out.
- `Mouse.down/up` send `x=0,y=0` and rely on the browser's last-known pointer position — a subtle coupling to prior `move`/`click`.

---

## Rust port notes

- **Maps cleanly.** The whole package is a stateless-ish command translator over CDP; it ports almost 1:1 onto the transport crate. `Page`/`Element`/`Mouse` become structs holding a client handle + ids; the [beta Rust bridge](11-beta-rust-bridge.md) already frames CDP-ish commands over JSON-RPC, so this facade is a natural client of that core.
- **Fallback ladders → typed enums.** Model each ladder as a `Result`-driven fallthrough (`getContentQuads` → `getBoxModel` → `getBoundingClientRect` → JS click) with an explicit `InteractionStrategy` enum instead of `except: pass`. This is a chance to *record* which rung fired (telemetry the Python version throws away).
- **String surgery is the hard part.** `_fix_javascript_string` and the arrow→function regex are heuristic and locale-fragile. In Rust, prefer a real tokenizer or force callers to pass a structured `{ params, body, is_async }` rather than re-deriving it with `regex` — the `([^)]*)` param capture and the escaped-quote counting are latent bugs worth designing out.
- **Keyboard tables.** `get_key_info` and the shifted-char maps are pure data — lift them into `const` lookup tables (`phf` for perfect hashing). The Unicode `upper()`-expansion guard needs care: Rust's `char::to_uppercase` also yields multi-`char` iterators, so replicate the "fall back to original code point" branch.
- **Concurrency.** Python leans on the single event loop for implicit ordering; a Rust port must decide whether actor handles borrow the session's lock or run lock-free, and make the Agent-vs-actor exclusion explicit rather than accidental.
