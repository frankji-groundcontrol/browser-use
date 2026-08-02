# Deploying `browser-use-rs` (build → install → verify)

The Rust MCP server all four coding agents run. Deploys must survive macOS
code-signing and be *verified*, not assumed — the ladder below caught a
SIGKILLed install and a request-ordering race on first use.

## Build + install

```bash
cd rust
# Verify BOTH feature configurations first: the release build has no
# `live-chrome`, so a helper that is gated behind it compiles in tests and
# breaks here. Ask for the exit code -- `cargo build | tail -1` reports tail's
# status, which once installed a STALE binary over a failed build.
cargo test --features live-chrome -- --test-threads=1
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features live-chrome -- -D warnings
cargo build --release          # must succeed on its own, unpiped

# NEVER `cp` over the live binary: macOS caches signatures per inode and
# SIGKILLs (exit 137) an in-place overwrite. Fresh inode + ad-hoc re-sign:
cp ~/.local/bin/browser-use-rs ~/.local/bin/browser-use-rs.prev-$(date +%Y%m%d)  # rollback
rm -f ~/.local/bin/browser-use-rs
install -m 755 target/release/browser-use-rs ~/.local/bin/browser-use-rs
codesign -f -s - ~/.local/bin/browser-use-rs
```

See [issues/2026-08-02-macos-sigkill-on-inplace-binary-overwrite.md](../issues/2026-08-02-macos-sigkill-on-inplace-binary-overwrite.md).

## Verification ladder

1. **Exec check** — `~/.local/bin/browser-use-rs --mcp </dev/null; echo $?`
   must print an rmcp error and exit `1`. Exit `137` = broken signature.
2. **Protocol probe** — pipe `initialize` + `tools/list` JSON-RPC lines over
   stdio; assert serverInfo `name=browser-use` and the expected tool count
   (17 as of 2026-08-02: 16 Python-identical + `browser_set_viewport`).
3. **Registration** — `claude mcp list` shows `✔ Connected`.
4. **Live smoke** — drive it as a *sequential* client (send one request, read
   its response, then send the next): navigate → `browser_screenshot` with
   `path` → `browser_set_viewport` → screenshot again, then open the saved
   images to eyeball the result (the mobile capture should show the page's
   media query actually applied).

## Gotcha: batched requests race

rmcp handles requests **concurrently**; the actor serializes in *arrival*
order. Writing several `tools/call` lines to stdin in one shot can apply a
viewport *after* the screenshot that was "sent" later. Real MCP clients await
each response, so this never bites in production — but a shell-piped smoke
test must do the same (small Python driver with `readline` per request).

## Rollback

Repoint to the kept `browser-use-rs.prev-YYYYMMDD` copy, or to the Python
wrapper per the rollback note in
[plans/2026-07-05-rust-rewrite/progress.md](../plans/2026-07-05-rust-rewrite/progress.md).
