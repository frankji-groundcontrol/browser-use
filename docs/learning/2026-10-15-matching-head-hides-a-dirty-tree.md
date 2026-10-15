# A matching HEAD hides a dirty tree on another host

Date: 2026-10-15

- "Sync with host X" looked like a no-op: this machine, the remote, and
  `origin/franky-rust` were all at the same commit. The remote's working tree
  held 15 files (+1451 lines) of uncommitted, unpushed work. This is the
  converse of the
  [stale installed binary](../issues/2026-08-26-installed-binary-drift-from-build-tree.md):
  there a clean tree hid old code, here an equal HEAD hid new code. Run
  `git status` on the remote before declaring any sync done.
- Eyeballing truncated `git status` output missed a fourth untracked file
  (`| head -15` cut it off). Enumerate with `git ls-files --others
  --exclude-standard` and let the tar list be the inventory.
- To move uncommitted work without touching the source host: `git diff
  --binary` for tracked changes, a tar of the untracked list, `git apply
  --check` as a dry run, then per-file sha256 on both ends. Equal hashes proved
  the copy; matching `git status` output would not have.
- A review workflow that returns 17 confirmed and 0 refuted is not 17 facts.
  Re-read the load-bearing ones yourself. Reading the code confirmed the
  blocker and four others; one claim (that a hand edit would fail CI) stayed
  only plausible until the generating script was read.

- The fix pass showed what mutation checks buy. A finding the reviewers ranked
  a nit (a hidden button still clicked after the snapshot) was a real regression:
  its failing test clicked the "Delete account" button. Conversely, a test that
  appeared to hang for ten minutes at 0% CPU under load ~21 passed in about 2 s
  on every rerun. Bound a live run with `perl -e 'alarm N; exec @ARGV' cargo test
  ...` instead of waiting, and rerun it before blaming the change.
- `cmd | tail; echo $?` reports `tail`'s status. A sync check that exited 1 read
  as 0 until it was rerun with its output redirected to a file. Read the exit code
  of the command, not of the pipeline. That check also failed for a reason
  unrelated to the hand edit it was suspected of (it compares against upstream's
  live `main`), so "CI will go red because of this edit" was true but incomplete.

Related: [review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md).
