# A clean `git status` hides a stale installed binary on a second host

Date: 2026-08-26 · Scope: keeping `browser-use-rs` in sync across two macOS hosts

## Problem

Asked to "sync browser-use on MBP2", every git-level signal said the work was
already done:

- `git status -sb` → clean, no modified files.
- `git rev-list --left-right --count HEAD...origin/franky-rust` → `0	0`.
- Same HEAD as the primary host (`b7f392e9c`).
- `cargo build -p bu-core --release` → **`Finished in 0.53s`** — nothing to
  rebuild, so the build tree was current too.

Yet the MCP server every agent on MBP2 actually executes was months-old code.
Git tracks the *source*; agents run `~/.local/bin/browser-use-rs`, and nothing
in the repo ever observes that path.

## Evidence

Hashing all three copies is what exposed it — sizes and mtimes actively mislead:

| Path | sha256 (first 16) | size | mtime |
| --- | --- | --- | --- |
| primary `rust/target/release/browser-use-rs` | `6beada385e5fc5ee` | 16574576 | Aug 16 12:44 |
| MBP2 `rust/target/release/browser-use-rs` | `915114e4cea69699` | 16574576 | Aug 16 12:06 |
| MBP2 `~/.local/bin/browser-use-rs` | `2c7f7d730225e726` | 16496320 | Aug 16 12:07 |

Two traps in that table:

- The stale installed copy has the **newest mtime** of the three (12:07 vs
  12:06), because it was deployed *from somewhere else* after MBP2's own build.
  Newer-looking, older code.
- The two hosts' build-tree binaries have **identical byte sizes** but different
  hashes — they are the same source built by different toolchains (cargo 1.97.1
  vs 1.96.0), so equal size is not equal content. Cross-host hash comparison is
  meaningless here; only *within* a host does hash equality prove currency.

## Root cause

`~/.local/bin/browser-use-rs` is a **copy** on MBP2 (on the primary host it is a
symlink into `rust/target/release/`, which cannot drift). A copy is a snapshot:
every later rebuild silently widens the gap, and no git or cargo command reports
it, because neither knows the deploy target exists.

## Resolution

Compare the installed binary against *its own host's* build tree, then redeploy
per [practices/deploy-browser-use-rs.md](../practices/deploy-browser-use-rs.md)
(fresh inode — never `cp` in place, see
[2026-08-02-macos-sigkill-on-inplace-binary-overwrite.md](2026-08-02-macos-sigkill-on-inplace-binary-overwrite.md)):

```bash
# The check that git cannot do for you, run per host:
shasum -a256 ~/.local/bin/browser-use-rs \
             ~/Softwares/browser-use/rust/target/release/browser-use-rs
```

Post-deploy the two hashes matched (`915114e4…`), `codesign -v` returned 0, and
the exec check exited `1` (not `137`), confirming a valid signature.

## Follow-up

The symlink layout used on the primary host makes this class of drift
structurally impossible, at the cost of breaking every agent if anyone runs
`cargo clean`. MBP2 was left as a copy to preserve its existing deploy style;
choosing one layout for both hosts is still open.
