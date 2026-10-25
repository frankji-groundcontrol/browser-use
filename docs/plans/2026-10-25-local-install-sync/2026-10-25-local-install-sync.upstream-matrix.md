# Upstream applicability matrix

Frozen source: upstream `3df5806a0`, common base `28670f720`, fork start
`e49186db1`. The fetched tracking branch matched the fork start. There are
361 upstream commits (100 first-parent), changing 157 files. This matrix maps
behavior into the existing Rust MCP/agent implementation; it does not claim
whole-library parity.

## Confirmed Rust ports

| Upstream contract | Rust change | Evidence |
| --- | --- | --- |
| Live values and checked state reach the agent (`5ffd3090a`, `3b12a946c`) | Include the existing redacted values in the autonomous agent observation | Real browser plus recorded model request: null-value RED, passing GREEN; secret input absent |
| Context for labelled and directly clickable images (`96117ef8f`, `81a472a3e`) | Keep existing labels and append bounded self/descendant image context | Pure DOM fixture contracts, then MCP live image regression |
| Browser-normalized image source privacy and bounded work (`469ed940e`, `81a472a3e`, `1cddc7a79`) | Exclude normalized data URLs; bound attributes, descendants and contexts | Oversized/control-character/deep-tree and credential-bearing authority fixture RED/GREEN |
| Hidden code display style normalization (`17cbc07ec`) | Shared HTML-to-Markdown extraction normalizes case and whitespace without matching unrelated attributes or CSS custom properties | Public extractor fixture RED/GREEN |
| Anthropic data scheme/MIME case (`ab806eba4`) and remote image preservation | One image-source builder serves simple and typed requests | Both public LLM request methods over local HTTP, RED/GREEN; remote URL control |
| Native date/time filling in Actor (`35d65335b`) | Port the behavior into the shared Rust indexed typing path | MCP date/time/month/week regression RED: success with empty date; GREEN for all five types; invalid values preserve form data |
| Disabled native select/option/optgroup validation (`95967882f`) | Reject disabled targets before changing form selection | MCP regression RED: disabled target returned success; GREEN rejects select/option/optgroup before mutation |
| Explicit launch-security setting (`72e53e3f3`) | Opt-in Chromium flags while retaining existing URL-policy interface | Real two-origin CORS RED/GREEN and launch argument tests |
| Empty headless environment value (`610c9614a`) | Empty explicitly means false; unset keeps the established Rust default | Parser table RED/GREEN |
| Reconnection resilience (`2c3162069`, `97c25801d`) | Ensure cached pages cannot bypass unhealthy-session recovery; check transport close behavior | Real attach/transport and owned-browser recovery tests GREEN; next-command bounded probe, no idle heartbeat or input replay |

## Already present or implemented differently

- CDP and MCP already capture live input/textarea values and checked state.
  Rust's earlier sensitivity filtering, autocomplete token handling and reveal
  protection are retained rather than replaced with narrower Python logic.
- Literal plus-key/chords and unknown-tool errors already have Rust MCP
  regressions. Failed tab switches propagate Result errors through MCP.
- Default launch argument ordering and fail-fast child exit are provided by
  Chromiumoxide. Failed partial attaches drop their owned session; Rust has no
  Python reconnect manager/event-bus registration to reset.
- Rust forwards caller JSON schemas and explicit endpoint/model/key settings;
  it has no schema optimizer, model alias classifier or provider-key fallback
  that could reproduce the corresponding upstream defects.
- OpenAI-compatible providers can use the generic chat transport. This does
  not imply native provider options, factories or cost-accounting parity.
- MCP SDK registration/property migrations are Python-specific. Rust uses
  rmcp and already exposes tool errors and read-only annotations.

## Explicit broader gaps

These are merged in the upstream Python tree; they are not silently labelled
as implemented Rust features:

- The new OpenAI/Claude Python SDK toolset adapters, Bash execution, scoped
  document references, approved remote file transfer and diagnostic APIs.
- Cloud browser provisioning, scoped cloud API keys/version negotiation,
  promotional/captcha hints and cloud task events. Rust supports explicit CDP
  attachment, which is a different contract.
- Native Gemini/Ollama adapters, provider convenience factories, arbitrary
  provider request fields, pricing lookup and provider-specific retry inputs.
- Agent file attachments/download registry, binary image file authoring,
  PDF/DOCX export, saved sensitive-data history maps and heuristic task URL
  extraction.
- Bedrock remote image downloading. Its Rust path consumes inline images;
  upstream's remote URL query/content-type fix is not proof of remote support.

## Merge resolution

Six historical fork/upstream conflicts were resolved by preserving the actual
contracts: pinned release actions plus fork tag/version gates; upstream's
scroll-before-measure and held-button Mouse with fork detached-node checks;
fork session attachment locking; one session initialization hook; upstream
extension configuration tests plus the fork download-timeout regression.
The project lockfile was regenerated to match merged metadata.

## Scope and verification

The user clarified Rust-only installation. The global Python CLI and other
hosts are outside scope. A project-local Python environment is used to validate
merge conflicts. No live model or paid cloud call is required.

See the [live plan](2026-10-25-local-install-sync.md) for final release, push,
installation and preservation evidence.

## Combined review repairs

One combined review found three further edge cases, each now RED/GREEN:
image clues use a parsed URL path and omit credential-bearing empty authority
paths; inline hidden-code matching identifies the actual display declaration;
page operations retain the exact page handle that passed URL policy validation.
The recovery fixture reproduced denied HTML content before the repair, and the
HTML and snapshot variants now pass. Snapshot selector indices become available
only after the complete capture succeeds.
