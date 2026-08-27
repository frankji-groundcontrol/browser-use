# MCP server: multi-agent setup

How to install browser-use as an MCP **server** and register it with several
coding agents at once (Claude Code, OpenAI Codex CLI, OpenCode, Hermes, Kimi
Code, Qoder, Grok), so each agent can drive a real browser through the same
tools.

> **What's deployed here.** The server in production on this host is the Rust
> reimplementation, **`browser-use-rs --mcp`** (crate workspace under
> [`rust/`](../../../rust), branch `franky-rust`) — a drop-in replacement for the
> Python `browser-use --mcp` with byte-identical `tools/list` output and full
> behavioural parity. See
> [architecture/12-rust-implementation.md](../../architecture/12-rust-implementation.md)
> for the design. The original Python setup is kept at the end as a rollback
> path. Secrets and private endpoints are shown as placeholders.

## The tool surface

`browser-use-rs --mcp` speaks MCP over stdio and exposes **19 tools** — 16 with
names/schemas byte-identical to the Python server, plus 3 additions:

- **14 low-level primitives** — `browser_navigate`, `browser_click`,
  `browser_type`, `browser_get_state`, `browser_get_html`, `browser_screenshot`,
  `browser_scroll`, `browser_go_back`, `browser_list_tabs`, `browser_switch_tab`,
  `browser_close_tab`, `browser_list_sessions`, `browser_close_session`,
  `browser_close_all`. These need **no LLM key**; the calling agent is the brain.
- **2 LLM-backed tools** — `browser_extract_content` (page → structured answer)
  and `retry_with_browser_use_agent` (a full autonomous sub-agent with vision,
  multi-action, and reasoning). These call the server's own OpenAI-compatible (or
  AWS Bedrock) model, so they need `BROWSER_USE_LLM_*` configured (§3).
- **3 fork-only additions** — `browser_set_viewport` (check responsive layouts
  at a real width without relaunching, so the session survives),
  `browser_read_clipboard` (capture text a page only exposes via a "Copy"
  button), and `browser_select_option` (drive native `<select>` dropdowns, which
  cannot be opened by synthetic mouse clicks under CDP). See
  [browser-sessions-and-login.md](browser-sessions-and-login.md).

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
`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome` build, or an explicit
path from the first set of `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH`,
`PLAYWRIGHT_CHROME_EXECUTABLE_PATH`, `CHROMIUM_PATH`, or `CHROME`. Provision one
once with either playwright or the Python package's `browser-use install`.

On a headless server the binary launches with `--no-sandbox` +
`--disable-dev-shm-usage` automatically (Ubuntu 23.10+/24.04's
`apparmor_restrict_unprivileged_userns=1` core-dumps Chrome otherwise). Headless
is the default; set `BROWSER_USE_HEADLESS=true` explicitly if you want to be sure.

Unlike the Python server, **each process gets its own unique `user_data_dir`**,
so multiple agents can drive browsers in parallel with no `SingletonLock`
contention.

## 3. Environment

The LLM is configured by four variables. Nothing else selects a backend — in
particular, exporting `OPENAI_API_KEY` for some other tool can no longer change
which model the agent talks to.

| Var | Purpose |
| --- | --- |
| `BROWSER_USE_LLM_BASE_URL` | Endpoint base. Used **exactly as given** for the first request. |
| `BROWSER_USE_LLM_API_KEY` | Credential. Sent as `Authorization: Bearer` (OpenAI) or `x-api-key` (Anthropic). |
| `BROWSER_USE_LLM_API` | Wire format: `openai-responses` (default), `openai-chat`, `anthropic-messages`, or `bedrock` (needs the `bedrock` build). |
| `BROWSER_USE_LLM_MODEL` | Model id. Required — there is no default. |
| `BROWSER_USE_LLM_TEMPERATURE` | Optional; defaults to `0.7`. |
| `BROWSER_USE_LLM_MAX_TOKENS` | Optional; defaults to `4096`. Required by Anthropic Messages, unused by the OpenAI formats. |

Route per format: `{base}/responses`, `{base}/chat/completions`, `{base}/messages`.

### Where the key lives

Each setting resolves through three layers, most explicit first:

1. **The process environment** — what an agent's MCP config passes. Still wins,
   so a single agent can be pinned to a different model or endpoint.
2. **`~/.config/browser-use/.env`** — the single source for everything else.
   Override the path with `BROWSER_USE_ENV_FILE`.
3. **The macOS Keychain**, service `browser-use-llm` — consulted for
   `BROWSER_USE_LLM_API_KEY` only, since it stores a password rather than a
   configuration.

So the normal setup is one file and **no secret in any agent's config**:

```bash
mkdir -p ~/.config/browser-use && chmod 700 ~/.config/browser-use
umask 077 && cat > ~/.config/browser-use/.env <<'EOF'
BROWSER_USE_LLM_BASE_URL=https://…/v1
BROWSER_USE_LLM_API=openai-responses
BROWSER_USE_LLM_MODEL=gpt-5.6-sol
BROWSER_USE_LLM_API_KEY=sk-…
EOF
```

To keep the key out of the filesystem entirely, omit it from the `.env` and put
it in the Keychain instead — it must be seeded from a **local terminal**, since a
non-interactive SSH session cannot unlock the login keychain:

```bash
security add-generic-password -U -a "$USER" -s browser-use-llm -w 'sk-…'
```

Rotation is then one command (or one file), not an edit across every agent.

> **Why this exists.** The key used to be copied literally into all five agent
> configs per host, and `~/.grok/config.toml` was created world-readable (0644)
> holding it. `grok`'s own `auth.json` is 0600, so the exposure was specific to
> the file that MCP env blocks land in. Check permissions after any
> `grok mcp add`: see
> [issues/2026-08-27-agent-config-world-readable-secrets.md](../../issues/2026-08-27-agent-config-world-readable-secrets.md).

**Base URL: exact first, `/v1` as a fallback.** The URL you set is the URL used.
Only if that route 404s — or answers 200 with an HTML landing page, which is how
gateways signal a wrong root — is the alternate root tried once (a bare host
gains `/v1`; a versioned one loses it). This is worth pinning correctly: on the
gateway in use here, `/v1/chat/completions` serves JSON while `/chat/completions`
returns the console's HTML, so setting the base to `https://…/v1` saves a wasted
round trip on every call.

**"Browser Use" means two different things — don't conflate them.** This
repository is the browser-automation *tool*. Browser Use also sells a hosted
*model* service (the `bu-*` family) at `llm.api.browser-use.com`, on a separate
account with its own key from `cloud.browser-use.com`. The tool does not require
it: point `BROWSER_USE_LLM_BASE_URL` at whatever model provider you already use.
A key for your own gateway will **not** authenticate against `llm.api.browser-use.com`,
and vice versa.

The `bu-*` models are ordinary LLMs, marketed as faster and cheaper for browser
tasks rather than more capable — so a frontier general model is a reasonable or
better choice, trading their speed for stronger reasoning.

If you *do* want that service, it needs no special provider — it is an
OpenAI-compatible endpoint:

```bash
BROWSER_USE_LLM_BASE_URL=https://llm.api.browser-use.com/v1
BROWSER_USE_LLM_API=openai-chat
BROWSER_USE_LLM_MODEL=bu-2-0
```

A 401 or 402 from that host is reported with the fix (invalid key / exhausted
credits and where to top up) rather than as a bare HTTP status.

Browser-side settings are unchanged:

| Var | Purpose |
| --- | --- |
| `BROWSER_USE_HEADLESS` | `true` for servers. |
| `BROWSER_USE_ALLOWED_DOMAINS` | Optional comma-separated allowlist; navigation off-list is blocked and disallowed pages are reset to `about:blank`. |
| `BROWSER_USE_PROHIBITED_DOMAINS` | Optional denylist (consulted when no allowlist is set). |
| `BROWSER_USE_BLOCK_IP_ADDRESSES` | `true` to reject bare-IP navigation (SSRF hardening). |
| `BROWSER_USE_CDP_URL` | Attach to an already-running Chrome instead of launching one. |
| `BROWSER_USE_USER_DATA_DIR` | Reuse a profile across runs so logins survive. |

> **Removed 2026-08-27.** `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `ANTHROPIC_API_KEY`,
> `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `MODEL_PROVIDER`,
> `BROWSER_USE_OPENAI_API`, and `BROWSER_USE_API_KEY` are no longer read at all.
> The old `ANTHROPIC_*` path never spoke Anthropic — it sent OpenAI-shaped bodies
> and only worked because those gateways serve both protocols; use
> `BROWSER_USE_LLM_API=anthropic-messages` for the real thing.

**No User-Agent workaround is needed.** The Rust client uses `reqwest`'s default
UA, which does not contain `OpenAI`, so gateways that WAF-block the official SDK's
fingerprint accept it directly. (This is why the Python wrapper existed — see §5.)

## 4. Register with each agent

All seven agents launch a local/stdio MCP server; point each at
`browser-use-rs --mcp`.

| Agent | Config file | Has an `mcp add` CLI |
| --- | --- | --- |
| Claude Code | `~/.claude.json` | yes |
| Codex CLI | `~/.codex/config.toml` | yes |
| OpenCode | `~/.config/opencode/opencode.json` | no |
| Hermes | `~/.hermes/config.yaml` | no |
| Grok | `~/.grok/config.toml` | yes |
| Qoder | `~/.qoder/settings.json` | yes |
| Kimi Code | `~/.kimi-code/mcp.json` | **no — edit the file** |

**Claude Code** (env values passed by `${VAR}` reference, expanded at spawn):

```bash
claude mcp add browser-use -s user \
  -e BROWSER_USE_LLM_API_KEY='${BROWSER_USE_LLM_API_KEY}' \
  -e BROWSER_USE_LLM_BASE_URL='${BROWSER_USE_LLM_BASE_URL}' \
  -e BROWSER_USE_LLM_API='openai-responses' \
  -e BROWSER_USE_LLM_MODEL='gpt-5.6-sol' -e BROWSER_USE_HEADLESS='true' \
  -- browser-use-rs --mcp
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.browser-use]
command = "browser-use-rs"
args = ["--mcp"]
[mcp_servers.browser-use.env]
BROWSER_USE_LLM_API_KEY = "…"
BROWSER_USE_LLM_BASE_URL = "https://…/v1"
BROWSER_USE_LLM_API = "openai-responses"
BROWSER_USE_LLM_MODEL = "gpt-5.6-sol"
BROWSER_USE_HEADLESS = "true"
```

**OpenCode** — top-level `mcp` object in `opencode.json`:

```json
"browser-use": {
  "type": "local",
  "command": ["browser-use-rs", "--mcp"],
  "enabled": true,
  "environment": {
    "BROWSER_USE_LLM_API_KEY": "{env:BROWSER_USE_LLM_API_KEY}",
    "BROWSER_USE_LLM_BASE_URL": "{env:BROWSER_USE_LLM_BASE_URL}",
    "BROWSER_USE_LLM_API": "openai-responses",
    "BROWSER_USE_LLM_MODEL": "gpt-5.6-sol"
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
    BROWSER_USE_LLM_API_KEY: "${BROWSER_USE_LLM_API_KEY}"
    BROWSER_USE_LLM_BASE_URL: "https://…/v1"
    BROWSER_USE_LLM_API: "openai-responses"
    BROWSER_USE_LLM_MODEL: "gpt-5.6-sol"
    BROWSER_USE_HEADLESS: "true"
```

**Grok** — `grok mcp add` writes `[mcp_servers.browser-use]` into
`~/.grok/config.toml`. Everything after `--` is the server command:

```bash
grok mcp add browser-use -s user \
  -e BROWSER_USE_LLM_API_KEY="$BROWSER_USE_LLM_API_KEY" \
  -e BROWSER_USE_LLM_BASE_URL="$BROWSER_USE_LLM_BASE_URL" \
  -e BROWSER_USE_LLM_API='openai-responses' -e BROWSER_USE_LLM_MODEL='gpt-5.6-sol' \
  -- ~/.local/bin/browser-use-rs --mcp
```

**Qoder** — `qoder mcp add` writes a `stdio` entry into `~/.qoder/settings.json`:

```bash
qoder mcp add browser-use -s user -t stdio \
  -e BROWSER_USE_LLM_API_KEY="$BROWSER_USE_LLM_API_KEY" \
  -e BROWSER_USE_LLM_BASE_URL="$BROWSER_USE_LLM_BASE_URL" \
  -e BROWSER_USE_LLM_API='openai-responses' -e BROWSER_USE_LLM_MODEL='gpt-5.6-sol' \
  -- ~/.local/bin/browser-use-rs --mcp
```

**Kimi Code** — no `mcp` subcommand, so merge into `~/.kimi-code/mcp.json`
directly (user-global; Kimi honours `$KIMI_CODE_HOME` first). Merge rather than
overwrite, so sibling servers survive:

```bash
F=~/.kimi-code/mcp.json
jq '.mcpServers["browser-use"] = {
      command: "/Users/me/.local/bin/browser-use-rs", args: ["--mcp"],
      env: {BROWSER_USE_LLM_API_KEY: env.BROWSER_USE_LLM_API_KEY,
            BROWSER_USE_LLM_BASE_URL: env.BROWSER_USE_LLM_BASE_URL,
            BROWSER_USE_LLM_API: "openai-responses",
            BROWSER_USE_LLM_MODEL: "gpt-5.6-sol"}
    }' "$F" > "$F.tmp" && mv "$F.tmp" "$F" && chmod 600 "$F"
```

`kimi doctor` validates `config.toml`/`tui.toml` but **not** `mcp.json`, so check
the server actually loaded with a one-shot prompt instead:
`kimi -p "List only the names of your available browser_* tools."` — expect 18
`browser_*` names (the 19th tool, `retry_with_browser_use_agent`, does not carry
the prefix).

Verify: `claude mcp get browser-use` (expect ✔ Connected), `codex mcp get
browser-use`, `hermes gateway restart && hermes gateway status`, `qoder mcp list`,
and `grok mcp doctor browser-use` (naming the server matters — a bare `grok mcp
doctor` probes *every* configured server and stalls on any unreachable one). A
quick manual smoke test — pipe `initialize` then `tools/list` into
`browser-use-rs --mcp` and expect 19 tools.

> `qoder mcp get <name>` prints the server's `env` block in plaintext, API key
> included. Prefer `qoder mcp list` when the output might be shared.

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
