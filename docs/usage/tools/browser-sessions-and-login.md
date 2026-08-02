# Sessions, logging in, and the clipboard

How to work with sites that need a login, and how to get text a page only hands
you via a "Copy" button. Applies to the Rust MCP server (`browser-use-rs --mcp`).

## The default session is transient

Every launch gets a throwaway Chromium profile that is **deleted when the
session closes**, and swept at the next launch if the process was killed. A
login therefore lasts exactly as long as the MCP server process — restart it and
you are logged out.

That is the privacy default: nothing outlives the session. It is *not* the same
as never touching disk — Chromium does write its (encrypted) cookie store while
running; it just does not survive. See
[issues/…-headless-incognito](../../issues/2026-08-02-headless-incognito-does-not-work.md)
for why a true off-the-record mode is not offered.

## Logging in

Headless is a display problem and profile lifetime is a persistence problem;
logging in usually needs both solved.

### One-off: just see the browser

```bash
BROWSER_USE_HEADLESS=false browser-use-rs --mcp
```

A visible window; log in by hand. The session still dies with the process.

### Recommended: log in once, stay logged in

`BROWSER_USE_USER_DATA_DIR` reuses a profile across runs and never deletes it.

```bash
# once, headful — log in by hand in the visible window
BROWSER_USE_HEADLESS=false \
BROWSER_USE_USER_DATA_DIR=~/.browser-use-rs/profile \
  browser-use-rs --mcp
```

Then point the MCP registration at the same directory; later **headless** runs
start authenticated:

```json
{ "env": { "BROWSER_USE_USER_DATA_DIR": "~/.browser-use-rs/profile" } }
```

A leading `~/` is expanded by the server — MCP env blocks are JSON and no shell
expands them.

This profile is isolated from your everyday browser, so the agent never sees
your normal passwords or history. Cookies for the sites you log into *are*
stored on disk; delete the directory to revoke.

**One profile, one browser.** Chromium holds a `SingletonLock` on a profile, so
two servers cannot share one directory concurrently — give each its own.

### Escape hatch: attach to your real browser

`BROWSER_USE_CDP_URL` attaches to a Chromium you started yourself, reusing its
profile and every login already in it.

```bash
# you launch Chrome
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --remote-debugging-port=9222
```

```json
{ "env": { "BROWSER_USE_CDP_URL": "http://127.0.0.1:9222" } }
```

Takes precedence over `BROWSER_USE_USER_DATA_DIR`. The browser is not the
server's: it is never closed and its profile is never deleted, so
`browser_close_session` / `browser_close_all` become no-ops for it.

> **Weigh this one.** The agent gets your real cookies and can act as you on
> every site you are signed into. Prefer a dedicated persistent profile unless
> you specifically need existing logins.

## Reading the clipboard

Some sites hand you text only through a **Copy** button — API keys, share links,
generated output — and never render it anywhere scrapeable. `browser_read_clipboard`
reads what that button wrote, optionally saving it:

```json
{"name": "browser_read_clipboard", "arguments": {"path": "/tmp/key.txt"}}
```

Returns the text plus `{"chars": N, "saved_to": "<absolute path>"}`.

Two limits, both inherent:

- **Secure context required.** The async clipboard API only exists on `https`,
  `http://localhost`, or `file` origins. On a `data:` URL `navigator.clipboard`
  is `undefined`.
- **Not the OS clipboard.** Headless Chromium keeps its own clipboard in-process,
  so this cannot read text you copied in another application — use `pbpaste`
  (macOS) or `xclip` (Linux) for that.

## LLM credentials for the two LLM-backed tools

`browser_extract_content` and `retry_with_browser_use_agent` call a model. The
other 16 tools need no key.

Resolution order — **whatever you configure explicitly always wins**:

1. `OPENAI_API_KEY` (+ `OPENAI_BASE_URL`, `BROWSER_USE_LLM_MODEL`)
2. otherwise `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY` + `ANTHROPIC_BASE_URL`

The second rule exists because Claude Code exports those from
`~/.claude/settings.json` into every process it spawns, so a server registered
with no `env` block works with no setup. Be aware that this means the tools
spend Claude Code's own gateway token. Setting `OPENAI_*` in the MCP `env` block
overrides it — verified: an explicit but unreachable `OPENAI_BASE_URL` fails
rather than silently falling back.

Codex inherits nothing: it authenticates by ChatGPT login (`auth.json` holds
OAuth tokens and a null `OPENAI_API_KEY`), so it always needs an explicit table:

```toml
[mcp_servers.browser-use.env]
OPENAI_API_KEY = "…"
OPENAI_BASE_URL = "http://your-gateway:8080"
BROWSER_USE_LLM_MODEL = "gpt-5.6-sol"
```

Requests use the **Responses API** (`POST {base}/responses`). Set
`BROWSER_USE_OPENAI_API=chat_completions` for gateways that only implement the
older route.

> **What the fallback actually requires.** `ANTHROPIC_BASE_URL` must point at an
> endpoint that serves the **OpenAI routes**, because this client posts
> `{base}/responses` (or `/chat/completions`) with `Authorization: Bearer`. It
> never speaks Anthropic's native protocol.
>
> That is not a stretch in practice: an Anthropic gateway typically serves the
> OpenAI routes too. Measured on the gateway this was tested against — the
> variables are Anthropic's, the wire protocol used is OpenAI's:
>
> | Route | Result |
> | --- | --- |
> | `/v1/messages` | Anthropic JSON (`content[].text`, `stop_reason`) |
> | `/v1/chat/completions`, `/v1/responses`, `/responses` | OpenAI JSON |
> | `/messages` | **HTTP 200 with an HTML landing page — not the API** |
>
> Note the asymmetry: the OpenAI routes answer at both roots, the Anthropic one
> only under `/v1`. A wrong base URL therefore does not fail cleanly, it returns
> 200 and HTML; this client reports it as a parse failure rather than silently
> returning nothing, but check the route your gateway actually serves.
>
> The real `api.anthropic.com` documents only `POST /v1/messages` with
> `x-api-key` + `anthropic-version`, so it would very likely NOT work — though
> that is inference, not measurement: probing it returned the same 403 edge
> block on every path, including one that certainly exists. Supporting it would
> mean writing a native Anthropic provider, which does not exist.

Whichever base URL you use, check the route it actually serves — some gateways
expose the OpenAI routes at the root (`/responses`) and others under `/v1`.

## Environment reference

| Variable | Effect |
| --- | --- |
| `BROWSER_USE_HEADLESS` | `false`/`0`/`no`/`off` shows the window. Default headless. |
| `BROWSER_USE_USER_DATA_DIR` | Reuse and keep this profile. Default: throwaway, deleted on close. |
| `BROWSER_USE_CDP_URL` | Attach to a running Chromium; takes precedence. |
| `OPENAI_API_KEY` / `OPENAI_BASE_URL` | LLM credentials; take precedence over everything else. |
| `BROWSER_USE_LLM_MODEL` | Model name for the LLM-backed tools. |
| `BROWSER_USE_OPENAI_API` | `responses` (default) or `chat_completions`. |
| `BROWSER_USE_ALLOWED_DOMAINS` | Restrict navigation. |
| `BROWSER_USE_COMMAND_TIMEOUT_MS` | Per-command backstop (default 90s). |
