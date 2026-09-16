# Lessons from the whole-repository review

- A green default test target proves only the default feature graph. Here the
  Python CI matrix passed, while Rust all-feature tests still failed when the
  browser actor channel closed.
- Documentation claims must be checked against source and runtime together:
  Rust exposes 19 MCP tools, while several architecture pages still say 16 and
  multiple Rust crates remain placeholders.
- Shell and workflow findings need a semantic repro. The missing `VERSION.txt`
  and absent lockfile were initially plausible Docker failures, but shell
  status masking and lock generation disprove the blanket claims; only the
  stale-base fast-image lock case remains.
- “Anonymized” telemetry can still carry raw tasks, URLs, action history and
  results. Data-minimization review must inspect serialized event properties,
  not just the setting name.
