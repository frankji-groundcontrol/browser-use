# Verify port contracts at every consumer

Date: 2026-10-25

## Lessons and evidence

- Restore the checkout's handoff before treating its directory name as an
  install target. The owner clarified that this checkout deploys a custom Rust
  MCP fork; the global Python CLI was outside scope.
- A captured field is not necessarily delivered to every caller. Live values
  and checked state existed in CDP/MCP, but the agent's separate observation
  projection dropped them. A real browser and captured model request reproduced
  the missing value before the two-field repair.
- Test every public serializer against one wire contract. Simple and typed
  Anthropic requests had separate data URL parsing and disagreed on HTTPS
  images. Shared conversion removed the cause after both paths failed the
  same independent request-body expectations.
- The presence of an environment variable name does not prove feature parity.
  The security variable already affected Rust URL policy but did not change
  Chromium CORS settings. A two-origin browser check discriminated those paths.
- Incoming paths disjoint from dirty files do not imply a conflict-free merge:
  committed fork changes can still conflict with upstream. Preserve a dirty-file
  baseline, inspect both sides, and rerun the retained behavior regressions.
- Distinguish an intermediate process snapshot from a completion result. The
  release runner had completed normally while a status snapshot showed no cargo
  process; the next wait returned every successful gate. Do not invent a stalled
  pipe diagnosis from missing progress output alone.
- Large-file gates should distinguish existing dependency source from new
  artifacts. The only over-1-MiB path was the already-tracked uv.lock and it shrank.
- Derive audit totals from the current files. The final link check found every
  target valid but a stale expected count failed after records added links;
  counts are evidence to measure, not fixed acceptance requirements.
- Syntax validation should target the merge risk. A generic duplicate-method
  assertion incorrectly rejected legitimate property accessors and overloads;
  parsing the files and checking the lifecycle hooks specifically established
  the intended invariant.

The single combined review also produced two verified public-output regressions:
URL authority/userinfo is not an image filename, and a CSS custom property is not
the actual display declaration. The repairs use parsed URL paths and declaration
boundaries rather than substring slicing. Recovery must likewise use the exact
page handle that passed policy validation; a subsequent reconnect may select a
different tab. The attached-Chrome proxy fixture returned denied content before
the fix; both HTML and snapshot variants passed after retaining the handle. This is a resource-identity boundary, not only a URL-string check.

Apply these checks when syncing a partial language port, changing protocol
serializers, or merging a long-lived fork. Record absent subsystems explicitly;
never call a missing API an already-satisfied fix.

See the [plan](../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.md),
[applicability matrix](../plans/2026-10-25-local-install-sync/2026-10-25-local-install-sync.upstream-matrix.md)
and [upstream practice](../practices/upstream-sync-and-port.md).
