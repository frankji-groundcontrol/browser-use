# OpenAI-compatible gateways can block the SDK by User-Agent

Date: 2026-07-05 · Scope: browser-use MCP server LLM tools behind a proxy gateway

## Lesson

When browser-use's two LLM-backed MCP tools are pointed at a ChatGPT-account
reverse-proxy gateway (an OpenAI-compatible endpoint that is not api.openai.com),
three independent things can break the LLM calls even though the key is valid:

1. The API is served under `/v1`. The `openai` SDK POSTs to
   `{base_url}/chat/completions` verbatim, so a base URL without `/v1` hits the
   gateway's web UI (HTML) and extraction silently returns "No content extracted".
2. The gateway serves a different model family. browser-use's default
   `gpt-4.1-mini` was rejected with *"model is not supported when using Codex with
   a ChatGPT account"*; the gateway served `gpt-5.x` models instead.
3. The gateway's WAF **403-blocks any request whose `User-Agent` contains
   `OpenAI`** — exactly the fingerprint the official SDK sends. A neutral UA
   (curl, Mozilla, `python-httpx`, or a custom string) passes; the
   `X-Stainless-*` headers are irrelevant.

## Evidence

- `GET {base}/v1/models` → JSON model list; `GET {base}/chat/completions` → HTML.
- `curl` with a plain UA → HTTP 200; the same request with
  `User-Agent: OpenAI/Python …` → HTTP 403 "Your request was blocked."
- browser-use's `ChatOpenAI` (which uses the SDK) → `PermissionDeniedError: Your
  request was blocked` until the UA was overridden, after which
  `browser_extract_content` and `retry_with_browser_use_agent` both succeeded.

## Fix / when to apply

Route the MCP server through
[`contrib/mcp/mcp-launch.py`](../../contrib/mcp/mcp-launch.py), which overrides
the SDK User-Agent via `default_headers`, optionally normalizes `/v1`, and sets
server-friendly defaults. `base_url` cannot be injected via
`~/.config/browseruse/config.json` because `LLMEntry` is not `extra='allow'`
(the field is dropped on load); rely on the SDK reading `OPENAI_BASE_URL` from
the environment instead. Apply whenever an OpenAI-compatible endpoint returns
403/"blocked", HTML instead of JSON, or "No content extracted" with a valid key.

## Related

- Headless-server sandbox crash (Ubuntu 24.04 unprivileged-userns restriction):
  set `chromium_sandbox: false`. See
  [usage/users](../usage/users/README.md).
