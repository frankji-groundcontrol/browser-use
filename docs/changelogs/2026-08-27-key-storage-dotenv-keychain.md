# Key storage: one `.env`, Keychain fallback, no secrets in agent configs

Date: 2026-08-27 · Issue: [agent configs held world-readable secrets](../issues/2026-08-27-agent-config-world-readable-secrets.md)

## What changed

`browser-use-rs` resolves every `BROWSER_USE_LLM_*` setting through three layers,
most explicit first:

1. **Process environment** — what an agent's MCP config passes. Still wins, so a
   single agent can be pinned to a different model or endpoint.
2. **`~/.config/browser-use/.env`** — the single source. Path overridable with
   `BROWSER_USE_ENV_FILE`.
3. **macOS Keychain**, service `browser-use-llm` — for `BROWSER_USE_LLM_API_KEY`
   only.

Agent configs were then stripped: four of the five per host now carry **no `env`
block at all**; the remainder keep only genuinely per-agent settings
(`BROWSER_USE_HEADLESS`, and one host's `BROWSER_USE_CDP_URL`).

## Why

The key was copied literally into five configs per host — ten plaintext copies,
and rotation meant editing ten files. `~/.grok/config.toml` was **0644** holding
it, alongside other MCP credentials. Details and the recurrence risk are in the
issue record.

## Implementation notes

- The `.env` parser tolerates `export ` prefixes, quotes, `#` comments, and
  whitespace around `=`. A quoted value runs to its **matching** closing quote, so
  `KEY='value'   # note` yields `value` and `KEY="has#hash"` keeps the `#`. The
  first attempt got this wrong — stripping the trailing comment before the quotes
  left the quotes attached — and the test caught it.
- The rule that the Keychain supplies *only* the secret lives in the layer
  composition, not inside the Keychain reader. Initially it was in the reader,
  which meant a test injecting a fake keychain bypassed the very policy it was
  asserting. Moving it made the composition the thing under test.
- Empty `.env` values are dropped rather than stored, so a blank line cannot mask
  a real value from a lower layer.
- Keychain lookups are cached in a `OnceLock` and only attempted for the secret,
  so the common path never shells out to `security`.

## Verification

- **49 `bu-llm` tests**, covering parser edge cases (quotes, comments, `export`,
  whitespace, unterminated quotes) and each precedence rule: env beats `.env`,
  `.env` beats Keychain, Keychain answers only for the API key.
- **Live, with an empty environment** on both hosts: `browser_extract_content`
  answered correctly reading purely from `.env`.
- **Keychain layer proven live**: pointing `BROWSER_USE_ENV_FILE` at a `.env` that
  deliberately omits the key still succeeded.
- After stripping, `grok mcp doctor browser-use` reports 19 tools on both hosts,
  and no active config contains the key.
- 27 secret-bearing files on the primary host and 30 on the secondary set to
  `0600`.

## Residual risk

The key is still plaintext in `~/.config/browser-use/.env` (0600). Keychain-only
storage works — omit the key from the `.env` — but the Keychain must be seeded
from a **local terminal**; a non-interactive SSH session cannot unlock the login
keychain, which is why the secondary host's Keychain entry is not yet seeded.
