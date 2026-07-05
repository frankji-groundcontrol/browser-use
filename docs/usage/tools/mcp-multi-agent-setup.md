# MCP server: multi-agent setup

How to install browser-use as an MCP **server** and register it with several
coding agents at once (Claude Code, OpenAI Codex CLI, OpenCode, Hermes), so each
agent can drive a real browser through the same 16 tools.

> **What's deployed here.** The server in production on this host is the Rust
> reimplementation, **`browser-use-rs --mcp`** (crate workspace under
> [`rust/`](../../../rust), branch `franky-rust`) — a drop-in replacement for the
> Python `browser-use --mcp` with byte-identical `tools/list` output and full
> behavioural parity. See
> [architecture/12-rust-implementation.md](../../architecture/12-rust-implementation.md)
> for the design. The original Python setup is kept at the end as a rollback
> path. Secrets and private endpoints are shown as placeholders.

## The tool surface

`browser-use-rs --mcp` speaks MCP over stdio and exposes **16 tools** (identical
names/schemas to the Python server):

- **14 low-level primitives** — `browser_navigate`, `browser_click`,
  `browser_type`, `browser_get_state`, `browser_get_html`, `browser_screenshot`,
  `browser_scroll`, `browser_go_back`, `browser_list_tabs`, `browser_switch_tab`,
  `browser_close_tab`, `browser_list_sessions`, `browser_close_session`,
  `browser_close_all`. These need **no LLM key**; the calling agent is the brain.
- **2 LLM-backed tools** — `browser_extract_content` (page → structured answer)
  and `retry_with_browser_use_agent` (a full autonomous sub-agent with vision,
  multi-action, and reasoning). These call the server's own OpenAI-compatible (or
  AWS Bedrock) model and need `OPENAI_API_KEY` (or `MODEL_PROVIDER=bedrock`).

## 1. Build & install the Rust binary

```bash
cd rust
cargo build -p bu-core --release           # -> rust/target/release/browser-use-rs
install -m755 target/release/browser-use-rs ~/.local/bin/browser-use-rs
```

Ensure `~/.local/bin` is on `PATH`. For AWS Bedrock support build with
`--features bedrock` (off by default so the OpenAI-compatible binary stays lean).

If a server is already running (agents keep the MCP subprocess alive), the copy
fails with *"Text file busy"*; unlink first so running processes keep the old
inode: `rm -f ~/.local/bin/browser-use-rs && cp … ~/.local/bin/browser-use-rs`,
then restart the agent to pick it up.

## 2. Chromium (headless-safe, auto-discovered)

Chromium is **not bundled**; the binary discovers a
`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome` build (or
`BROWSER_USE_CHROMIUM_PATH`). Provision one once with either playwright or the
Python package's `browser-use install`.

On a headless server the binary launches with `--no-sandbox` +
`--disable-dev-shm-usage` automatically (Ubuntu 23.10+/24.04's
`apparmor_restrict_unprivileged_userns=1` core-dumps Chrome otherwise). Headless
is the default; set `BROWSER_USE_HEADLESS=true` explicitly if you want to be sure.

Unlike the Python server, **each process gets its own unique `user_data_dir`**,
so multiple agents can drive browsers in parallel with no `SingletonLock`
contention.

## 3. Environment

| Var | Purpose |
| --- | --- |
| `OPENAI_API_KEY` | Bearer auth for the 2 LLM tools. |
| `OPENAI_BASE_URL` | **Must include the API path** (e.g. `https://…/v1`) — the client POSTs `{base}/chat/completions`. A bare host hits the gateway's HTML page → *"failed to parse LLM chat response"*. |
| `BROWSER_USE_LLM_MODEL` | Model id (default `gpt-4o`; set to what your gateway lists). |
| `BROWSER_USE_LLM_TEMPERATURE` | Optional; defaults to `0.7`. |
| `BROWSER_USE_HEADLESS` | `true` for servers. |
| `BROWSER_USE_ALLOWED_DOMAINS` | Optional comma-separated allowlist; navigation off-list is blocked and disallowed pages are reset to `about:blank`. |
| `BROWSER_USE_PROHIBITED_DOMAINS` | Optional denylist (consulted when no allowlist is set). |
| `BROWSER_USE_BLOCK_IP_ADDRESSES` | `true` to reject bare-IP navigation (SSRF hardening). |
| `MODEL_PROVIDER=bedrock` | Use AWS Bedrock (requires the `bedrock` build); `MODEL`/`REGION` select the model. |

**No User-Agent workaround is needed.** The Rust client uses `reqwest`'s default
UA, which does not contain `OpenAI`, so gateways that WAF-block the official SDK's
fingerprint accept it directly. (This is why the Python wrapper existed — see §5.)

## 4. Register with each agent

All four agents launch a local/stdio MCP server; point each at
`browser-use-rs --mcp`.

| Agent | Config file |
| --- | --- |
| Claude Code | `~/.claude.json` |
| Codex CLI | `~/.codex/config.toml` |
| OpenCode | `~/.config/opencode/opencode.json` |
| Hermes | `~/.hermes/config.yaml` |

**Claude Code** (env values passed by `${VAR}` reference, expanded at spawn):

```bash
claude mcp add browser-use -s user \
  -e OPENAI_API_KEY='${OPENAI_API_KEY}' -e OPENAI_BASE_URL='${OPENAI_BASE_URL}' \
  -e BROWSER_USE_LLM_MODEL='gpt-5.4-mini' -e BROWSER_USE_HEADLESS='true' \
  -- browser-use-rs --mcp
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.browser-use]
command = "browser-use-rs"
args = ["--mcp"]
[mcp_servers.browser-use.env]
OPENAI_API_KEY = "…"
OPENAI_BASE_URL = "https://…/v1"
BROWSER_USE_LLM_MODEL = "gpt-5.4-mini"
BROWSER_USE_HEADLESS = "true"
```

**OpenCode** — top-level `mcp` object in `opencode.json`:

```json
"browser-use": {
  "type": "local",
  "command": ["browser-use-rs", "--mcp"],
  "enabled": true,
  "environment": {
    "OPENAI_API_KEY": "{env:OPENAI_API_KEY}",
    "OPENAI_BASE_URL": "{env:OPENAI_BASE_URL}",
    "BROWSER_USE_LLM_MODEL": "gpt-5.4-mini"
  }
}
```

**Hermes** — top-level `browser-use` under the MCP servers key in `config.yaml`:

```yaml
browser-use:
  command: browser-use-rs
  args: [--mcp]
  enabled: true
  env:
    OPENAI_API_KEY: "${OPENAI_API_KEY}"
    OPENAI_BASE_URL: "https://…/v1"
    BROWSER_USE_LLM_MODEL: "gpt-5.4-mini"
    BROWSER_USE_HEADLESS: "true"
```

Verify: `claude mcp get browser-use` (expect ✔ Connected), `codex mcp get
browser-use`, `hermes gateway restart && hermes gateway status`. A quick manual
smoke test — pipe `initialize` then `tools/list` into `browser-use-rs --mcp` and
expect 16 tools.

MCP servers run as agent subprocesses. After reinstalling the binary or changing
env, **restart the agent session** (or its gateway, e.g. `hermes gateway
restart`) so it respawns the new binary — a long-lived subprocess keeps the old
one until then.

## 5. Rollback to the Python server

The Python install is retained for rollback. Repoint an agent's `browser-use`
command back to the Python wrapper, which patches the OpenAI SDK's User-Agent,
optionally appends `/v1` (`BROWSER_USE_MCP_FORCE_V1=1`), and defaults to
headless:

```bash
claude mcp remove browser-use -s user
claude mcp add browser-use -s user \
  -e OPENAI_API_KEY='${OPENAI_API_KEY}' -e OPENAI_BASE_URL='${OPENAI_BASE_URL}' \
  -- ~/.local/share/uv/tools/browser-use/bin/python ~/.config/browseruse/mcp-launch.py
```

Background on the three gateway obstacles the wrapper fixes (`/v1` path, model
family, User-Agent WAF):
[learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md](../../learning/2026-07-05-openai-gateway-sdk-fingerprint-block.md).
