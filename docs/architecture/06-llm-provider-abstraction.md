# LLM Provider Abstraction & Structured Output

A provider-agnostic async layer over ~15 chat-completion backends behind one
narrow contract: `ainvoke(messages, output_format) -> ChatInvokeCompletion`. It
normalizes messages, coerces structured output per provider, and reports token
usage. Source: [`browser_use/llm/`](../../browser_use/llm/).

## The contract: `BaseChatModel`

[`base.py`](../../browser_use/llm/base.py) defines `BaseChatModel` as a
`@runtime_checkable` `Protocol` (not an ABC) — any object with the right shape
satisfies it, and `__get_pydantic_core_schema__` lets it be embedded as a typed
field in pydantic models (e.g. agent settings).

```python
@runtime_checkable
class BaseChatModel(Protocol):
    model: str
    @property
    def provider(self) -> str: ...
    @property
    def name(self) -> str: ...
    @overload
    async def ainvoke(self, messages: list[BaseMessage], output_format: None = None, **kw) -> ChatInvokeCompletion[str]: ...
    @overload
    async def ainvoke(self, messages: list[BaseMessage], output_format: type[T], **kw) -> ChatInvokeCompletion[T]: ...
```

The `@overload` pair encodes the key behavior: **no `output_format` → a string
completion; a pydantic `type[T]` → a validated `T`**. `T` is bound to
`BaseModel`.

## Messages and results

- [`messages.py`](../../browser_use/llm/messages.py) — the provider-agnostic
  message model (`BaseMessage` and friends), replacing the former LangChain types
  (see the module docstring). Each provider serializer maps these to its wire
  format.
- [`views.py`](../../browser_use/llm/views.py):
  - `ChatInvokeCompletion[T]` — `completion: T`, plus `thinking` /
    `redacted_thinking` (reasoning models), `usage`, `stop_reason`,
    `stop_details`.
  - `ChatInvokeUsage` — `prompt_tokens`, `prompt_cached_tokens`,
    `prompt_cache_creation_tokens` (+ Anthropic 5m/1h cache-write splits),
    `prompt_image_tokens` (Google), `completion_tokens`, `total_tokens`, and a
    `pricing_multiplier` for provider-specific cost (e.g. Anthropic US-only
    inference). Consumed by [`tokens/`](../../browser_use/tokens/) — see
    [10-cross-cutting-services.md](10-cross-cutting-services.md).

## Structured output: `SchemaOptimizer`

[`schema.py`](../../browser_use/llm/schema.py) — `SchemaOptimizer.create_optimized_json_schema(model, remove_min_items=, remove_defaults=)`
takes a pydantic model's `model_json_schema()` and:

- **flattens all `$ref`/`$defs`** inline (many providers choke on refs) while
  preserving full descriptions and every action definition,
- strips `additionalProperties`/`$defs`,
- makes the schema **OpenAI strict-mode compatible**.

This is the single most reused piece across providers and, being pure
JSON-tree-walking, the easiest to port.

## Per-provider structured-output divergence

There is no single "structured output" mechanism; each provider needs different
handling. Example from [`openai/chat.py`](../../browser_use/llm/openai/chat.py):

- Builds a strict `response_format` JSON schema
  (`ResponseFormatJSONSchema(json_schema={strict: True, schema: SchemaOptimizer...})`).
- `add_schema_to_system_prompt` option injects the schema as text instead of
  using `response_format` (for endpoints that don't support it).
- `reasoning_models` list → sets `reasoning_effort` (default `'low'`) and adjusts
  token accounting (reasoning_tokens are a subset of completion_tokens).
- On unparseable output, raises `ModelProviderError('Failed to parse structured
  output…')`.

Other families diverge: Anthropic uses **forced `tool_choice`** + cache_control
placement + thinking-block extraction; Google Gemini uses `response_schema` (with
a Gemini-specific SchemaOptimizer variant). The providers live in per-backend
subpackages: `openai`, `anthropic`, `aws` (Bedrock), `azure`, `google`, `groq`,
`mistral`, `cerebras`, `deepseek`, `openrouter`, `ollama`, `vercel`, `litellm`,
`oci_raw`, and `browser_use` (the ChatBrowserUse cloud gateway).

## Error taxonomy

[`exceptions.py`](../../browser_use/llm/exceptions.py): `ModelError` →
`ModelProviderError` (has `status_code`, `model`) → `ModelRateLimitError`. The
agent's fallback-LLM logic and retry loop key off these (see
[05-agent-control-loop.md](05-agent-control-loop.md)).

## Rust port notes

- The Protocol → `#[async_trait] trait ChatModel` with split
  `ainvoke`/`ainvoke_structured<T: DeserializeOwned>`. See
  [porting-map.md](../plans/2026-07-05-rust-rewrite/porting-map.md).
- `async-openai` (base_url) collapses the entire OpenAI-compatible family into one
  client; Anthropic + Gemini are hand-rolled over `reqwest` + `serde` (get
  cache_control, forced tool_choice, and thinking-block extraction right).
- `SchemaOptimizer` is a pure `serde_json::Value` tree-walk — a clean, golden-file
  testable first port.
- serde is **stricter** than Python's `json`, so the JSON-repair heuristics matter
  *more*, not less; port them with unit tests. Structured output returns to
  `ChatInvokeCompletion<T>` with `usage`/`thinking`/`stop_reason` parity.

See also [index.md](index.md) and the LLM subsystem's own
[`browser_use/llm/README.md`](../../browser_use/llm/README.md).
