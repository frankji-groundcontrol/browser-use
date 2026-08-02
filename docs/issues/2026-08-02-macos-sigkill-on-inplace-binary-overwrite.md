# macOS SIGKILLs a binary overwritten in place (`cp` over the live file)

Date: 2026-08-02 · Scope: deploying `browser-use-rs` to `~/.local/bin`

## Problem

Redeploying with `cp target/release/browser-use-rs ~/.local/bin/browser-use-rs`
produced a binary that died instantly with **exit 137 (SIGKILL)** and no output;
every MCP client showed `✘ Failed to connect — Connection closed`. The same
binary ran fine from `target/release/`.

## Evidence

- `~/.local/bin/browser-use-rs --mcp </dev/null; echo $?` → `137`, zero output.
- The build-tree copy of the identical bytes exited normally.
- `codesign -v` on the installed file reported an invalid signature.

## Root cause

`cp` onto an existing file rewrites the **same inode**. macOS caches code
signatures per inode; overwriting the contents invalidates the recorded
signature, and the kernel kills the process at exec with SIGKILL. This only
bites when replacing a previously-executed binary, which is exactly the
redeploy case.

## Resolution

Never overwrite in place. Replace the file so it gets a fresh inode, then
re-sign ad hoc:

```bash
rm -f ~/.local/bin/browser-use-rs
install -m 755 rust/target/release/browser-use-rs ~/.local/bin/browser-use-rs
codesign -f -s - ~/.local/bin/browser-use-rs
```

See [practices/deploy-browser-use-rs.md](../practices/deploy-browser-use-rs.md)
for the full deploy + verification sequence.
