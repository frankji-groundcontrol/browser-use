# Agent MCP configs held API keys, one of them world-readable

Date: 2026-08-27 · Scope: agent config files holding `mcp_servers.*.env` secrets

## Problem

Registering an MCP server with a `-e KEY=value` flag writes the value **literally**
into that agent's config file. With seven agents across two hosts, one gateway
key existed as ten plaintext copies, and rotating it meant editing ten files.

Worse, `~/.grok/config.toml` was mode **0644** — world-readable — while holding
that key plus a docmind bearer token and other MCP credentials. Any process or
user on the machine could read them.

## Evidence

- All four `~/.grok/config.toml.bak-*` snapshots, the oldest from 2026-08-24 and
  predating this task, are also `-rw-r--r--`. The exposure was **not** introduced
  by the migration that surfaced it.
- `grok`'s own `~/.grok/auth.json` and `~/.grok/agent_id` are `-rw-------`. So the
  agent does write private files when it considers something a secret; the gap is
  specific to `config.toml`, which is where MCP `env` blocks land.
- `~/.codex/config.toml`, also agent-written and also holding the key, is `0600` —
  so this is a per-agent oversight, not a platform convention.
- A sweep for files containing a key or bearer token found **12 world-readable
  files** on the primary host, most of them `.bak-*` snapshots left by earlier
  edits. Backups inherit the mode of whatever created them, so a single
  world-readable original propagates.

## Root cause

Two compounding causes:

1. `grok` creates `config.toml` with default permissions rather than `0600`,
   despite that file being the destination for MCP credentials.
2. The MCP config file is the *only* place these agents can hold a credential, so
   secrets were duplicated per agent by design. There was no single source.

## Resolution

Both addressed:

- **Permissions:** `chmod 600` applied to every secret-bearing config and backup
  on both hosts (27 files on the primary, 30 on the secondary).
- **Single source:** `browser-use-rs` now resolves settings from the process
  environment, then `~/.config/browser-use/.env`, then the macOS Keychain for the
  API key. Every agent config was stripped of the key; four of the five now carry
  no `env` block at all. See
  [the changelog](../changelogs/2026-08-27-key-storage-dotenv-keychain.md).

Verified after stripping: with an empty environment the server still answered a
`browser_extract_content` call on both hosts, and a `.env` deliberately missing
the key succeeded via the Keychain.

## This will recur

`grok mcp add` will create or rewrite `config.toml` with default permissions
again. It is not fixed upstream, and nothing in this repo can prevent it.

**Check after any `grok mcp add`:**

```bash
stat -f "%Sp %N" ~/.grok/config.toml   # expect -rw-------
chmod 600 ~/.grok/config.toml
```

The same caution applies to any agent that writes credentials to a config file —
verify the mode rather than assuming the tool chose a safe one. Note also that
`qoder mcp get <name>` prints the full `env` block, secrets included, so prefer
`qoder mcp list` when output may be shared.

## Residual risk

The key remains in plaintext in `~/.config/browser-use/.env` (0600) on both
hosts. Moving it into the Keychain entirely is supported — omit it from the
`.env` — but the Keychain must be seeded from a local terminal, because a
non-interactive SSH session cannot unlock the login keychain. That step is
outstanding on the secondary host.
