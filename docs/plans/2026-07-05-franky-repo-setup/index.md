# Plan: franky repo setup (2026-07-05)

Status: **in progress**

Umbrella plan for standing up the `franky` branch on the
`frankji-groundcontrol/browser-use` fork: repoint the remote, capture the local
MCP install work as real docs, and organize the repository documentation per the
`clean-repo-org` practice. The Rust rewrite has its own plan
([2026-07-05-rust-rewrite](../2026-07-05-rust-rewrite/index.md)).

Privacy risk: this branch is on a **public** fork. No secrets, tokens, or
private gateway hostnames are committed; gateway specifics are described
generically with placeholders.

## Checklist

- [x] Repoint `origin` → fork; keep upstream; create + push `franky`.
- [x] Verify the local install (all 16 MCP tools, incl. both LLM-backed tools).
- [x] Add the launcher `contrib/mcp/mcp-launch.py` (generic, env-driven).
- [x] Write the MCP multi-agent setup guide (`docs/usage/tools/`).
- [x] Write the gateway-block learning record (`docs/learning/`).
- [x] Bootstrap the `docs/` skeleton + indexes (usage/issues/learning/plans/practices).
- [ ] Write real architecture docs from the codebase deep-read
      (`docs/architecture/*`).
- [ ] Add thin documentation-pointer sections to `CLAUDE.md` / `AGENTS.md`.
- [ ] Verify all internal links resolve; commit + push to `franky`.

## Log

- Repo remote repointed to the fork; `franky` branch created and pushed.
- Install verified end-to-end via stdio MCP smoke tests (navigate, extract,
  autonomous agent) against a headless Chromium with `chromium_sandbox: false`.
- Docs skeleton + install/gateway docs written; architecture docs pending a
  background deep-read of all modules.
