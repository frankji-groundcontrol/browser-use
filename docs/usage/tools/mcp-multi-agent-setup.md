# MCP server: multi-agent setup

How to install browser-use as an MCP **server** and register it with several
coding agents at once (Claude Code, OpenAI Codex CLI, OpenCode, Hermes), so each
agent can drive a real browser through the same 16 tools.

This guide records a working setup on a headless Ubuntu 24.04 host, including the
two non-obvious obstacles you will hit there: Chrome's sandbox on modern Ubuntu,
and OpenAI-compatible gateways that block the official SDK. Secrets and private
endpoints are shown as placeholders — substitute your own.

## The tool surface

`browser-use --mcp` speaks MCP over stdio and exposes **16 tools**:

- **14 low-level primitives** — `browser_navigate`, `browser_click`,
  `browser_type`, `browser_get_state`, `browser_get_html`, `browser_screenshot`,
  `browser_scroll`, `browser_go_back`, `browser_list_tabs`, `browser_switch_tab`,
  `browser_close_tab`, `browser_list_sessions`, `browser_close_session`,
  `browser_close_all`. These need **no LLM key**; the calling agent is the brain.
- **2 LLM-backed tools** — `browser_extract_content` (page → structured answer)
  and `retry_with_browser_use_agent` (a full autonomous sub-agent). These use the
  server's own OpenAI-compatible model and therefore need `OPENAI_API_KEY`.

## 1. Install

```bash
uv tool install 'browser-use[cli]'   # -> ~/.local/bin/browser-use (+ aliases bu, browser)
```

The `[cli]` extra is currently empty, so `uv tool install browser-use` is
equivalent. Ensure `~/.local/bin` is on `PATH` (`uv tool update-shell`).

## 2. Provision Chromium (headless-safe)

Chromium is downloaded, not bundled:

```bash
browser-use install     # playwright install chromium (+ --with-deps on Linux)
```

On a **headless server** set headless mode and disable the Chrome sandbox — on
Ubuntu 23.10+/24.04, `apparmor_restrict_unprivileged_userns=1` makes Chrome
**core-dump** unless it launches with `--no-sandbox`. browser-use adds
`--no-sandbox` when `chromium_sandbox` is false. Edit
`~/.config/browseruse/config.json` and set on the default browser profile:

```json
{ "headless": true, "chromium_sandbox": false }
```

Because `LLMEntry`/`BrowserProfileEntry` differ (only the profile entry is
`extra='allow'`), `chromium_sandbox` propagates through config but `base_url`
does **not** — see §4.

Verify end to end:

```bash
browser-use doctor
```

## 3. Register with each agent

All four agents support local/stdio MCP servers. None ship a `browser-use` entry
by default, so there is no collision.

| Agent | Mechanism | Config file |
| --- | --- | --- |
| Claude Code | `claude mcp add … -s user` | `~/.claude.json` |
| Codex CLI | `codex mcp add …` | `~/.codex/config.toml` |
| OpenCode | edit `mcp` object (CLI is interactive) | `~/.config/opencode/opencode.json` |
| Hermes | edit `mcp_servers` (or `hermes mcp add`) | `~/.hermes/config.yaml` |

**Claude Code** (env values passed by `${VAR}` reference, expanded at spawn):

```bash
claude mcp add browser-use -s user \
  -e OPENAI_API_KEY='${OPENAI_API_KEY}' -e OPENAI_BASE_URL='${OPENAI_BASE_URL}' \
  -- <python-with-browser-use> <path>/contrib/mcp/mcp-launch.py
```

**Codex CLI**:

```bash
codex mcp add browser-use \
  --env OPENAI_API_KEY="$OPENAI_API_KEY" --env OPENAI_BASE_URL="$OPENAI_BASE_URL" \
  -- <python-with-browser-use> <path>/contrib/mcp/mcp-launch.py
```

**OpenCode** — add to the top-level `mcp` object:

```json
"browser-use": {
  "type": "local",
  "command": ["<python-with-browser-use>", "<path>/contrib/mcp/mcp-launch.py"],
  "enabled": true,
  "environment": { "OPENAI_API_KEY": "{env:OPENAI_API_KEY}", "OPENAI_BASE_URL": "{env:OPENAI_BASE_URL}" }
}
```

**Hermes** — add a top-level `mcp_servers` key (values interpolate `${VAR}` from
the environment):

```yaml
mcp_servers:
  browser-use:
    command: <python-with-browser-use>
    args: [<path>/contrib/mcp/mcp-launch.py]
    enabled: true
    env:
      OPENAI_API_KEY: "${OPENAI_API_KEY}"
      OPENAI_BASE_URL: "${OPENAI_BASE_URL}"
```

Verify: `claude mcp get browser-use`, `codex mcp get browser-use`,
`hermes mcp test browser-use` (expect “16 tools”), `opencode mcp list`.

## 4. Gateways that block the OpenAI SDK

If your `OPENAI_BASE_URL` points at a ChatGPT-account reverse-proxy gateway
rather than api.openai.com, three things commonly break the two LLM-backed tools
(the 14 low-level tools are unaffected):

1. **`/v1` path** — the SDK POSTs to `{base}/chat/completions`; if the gateway
   serves its API under `/v1`, the base URL must end in `/v1`.
2. **Model** — the gateway may reject browser-use's default (`gpt-4.1-mini`) and
   serve a different family; set `BROWSER_USE_LLM_MODEL` to one it lists at
   `GET {base}/v1/models`.
3. **User-Agent WAF** — some gateways 403-block any request whose `User-Agent`
   contains `OpenAI` (the SDK's fingerprint). browser-use's MCP server offers no
   header hook.

The launcher [`contrib/mcp/mcp-launch.py`](../../../contrib/mcp/mcp-launch.py)
fixes all three: it patches `openai.AsyncOpenAI` to send a neutral User-Agent,
optionally appends `/v1` (`BROWSER_USE_MCP_FORCE_V1=1`), defaults to headless,
then runs the normal server. Point each agent's command at it (as shown above)
instead of at `browser-use` directly. It is fully env-driven and hardcodes
nothing gateway-specific. Background and evidence:
[learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md](../../learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md).

## Notes & caveats

- The LLM tools need each agent's **process** to have `OPENAI_API_KEY` /
  `OPENAI_BASE_URL` in its environment at launch (the configs above pass them by
  reference). Without them, the 14 low-level tools still work.
- All agents share one Chrome profile
  (`~/.config/browseruse/profiles/default`). One agent driving the browser at a
  time is fine; concurrent use contends on Chrome's `SingletonLock`. Give each
  agent its own `user_data_dir` (via a per-agent `BROWSER_USE_CONFIG_DIR`) for
  true parallelism.
- Restart a running agent session (or use its reload command) to pick up config
  changes.
