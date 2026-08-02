# Chromium profiles leaked: 24 dirs / 390 MB, because `Drop` cannot delete them

Date: 2026-08-02 · Scope: Rust port, `bu-cdp` throwaway user-data dirs

## Problem

Each session creates a throwaway profile under `TMPDIR` and deletes it in
`Drop`. Measured reality on this machine: **24 abandoned profiles totalling
390 MB**. The removal is `let _ = fs::remove_dir_all(...)`, so every failure was
silent.

Surfaced by a new test asserting the profile is gone after the session drops —
it failed immediately, then `du` confirmed the accumulation.

## Root cause

Deleting in `Drop` cannot work here, for two independent reasons:

1. **The browser is still alive.** chromiumoxide's `Browser::drop` does not kill
   and wait; it relies on `kill_on_drop` and lets the async runtime reap the
   child **in the background**. So when our `Drop` runs, Chromium still holds
   the profile and is still writing to it — `remove_dir_all` races and loses.
2. **A killed process never runs `Drop` at all.** SIGKILL, a crashed host, or a
   `TaskStop` leaves the directory behind with no cleanup path whatsoever.

## Resolution

Startup garbage collection, keyed on an OS-held lock rather than on a pid or a
timestamp:

- Each scratch profile gets a `.bu-owner.lock` file, exclusively locked with
  `std::fs::File::try_lock` (stable since Rust 1.89 — no new dependency) and
  held for the session's lifetime.
- `unique_user_data_dir()` first sweeps `TMPDIR`: for every
  `browser-use-rs-chromium-*` directory, it tries to take that lock. **Success
  means the owner is gone**, so the directory is removed; failure means a live
  session (possibly another agent's) and it is skipped.

The OS releases the lock however the process dies, which is exactly the property
a pid check or an mtime heuristic lacks. Reclaimed all 390 MB.

`Drop` still attempts removal — it usually succeeds once `close()` has run, and
the sweep is the backstop.

## Related fix

`BrowserSession::close()` now awaits `Browser::wait()` instead of returning as
soon as the close request is acknowledged. Chromium holds a `SingletonLock` on
its profile, so relaunching against a **persistent** profile too soon silently
yields a browser with none of the stored cookies. That is precisely how the
cookie-persistence test failed under full-suite load while passing in isolation.
