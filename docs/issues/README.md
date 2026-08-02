# Issue records

Concrete, dated records of implementation issues found and how they were
resolved (or deferred). Keep each record scoped to one problem with evidence and
a resolution.

## Index

- [2026-08-02 — DOMSnapshot bounds are frame-local: iframe clicks hit the wrong element](2026-08-02-iframe-frame-local-bounds-wrong-click.md)
  — the Rust port's highest-severity finding: wrong-element clicks and
  everything-culled occlusion, fixed via `getContentQuads` (+ forced reflow)
  and per-`frameId` occlusion scoping.
- [2026-08-02 — Off-the-record mode does not work under headless Chromium](2026-08-02-headless-incognito-does-not-work.md)
  — negative result: both `createBrowserContext` and `--incognito` verified
  ineffective, and the encryption-at-rest trap that made the first test
  falsely pass.
- [2026-08-02 — Chromium profiles leaked: 24 dirs / 390 MB](2026-08-02-chromium-profile-leak-and-lock-gc.md)
  — `Drop` cannot delete a profile the browser still holds (and never runs on
  SIGKILL); fixed with an OS-held lock file and a startup sweep.
- [2026-08-02 — CDP viewport-override quirks](2026-08-02-cdp-viewport-override-quirks.md)
  — a 0x0 `setDeviceMetricsOverride` does *not* clear (use
  `clearDeviceMetricsOverride`), and `mobile:true` on a non-responsive page
  lays out at Chrome's 980px fallback.
- [2026-08-02 — macOS SIGKILLs a binary overwritten in place](2026-08-02-macos-sigkill-on-inplace-binary-overwrite.md)
  — `cp` over the live `browser-use-rs` → exit 137; deploy with
  `rm` + `install` + `codesign`.
- [2026-08-02 — `tests/ci` wedge: proxy env + unbounded extension download](2026-08-02-tests-ci-wedge-proxy-and-extension-download.md)
  — two stacked root causes behind a 25-minute silent hang; the "sandbox
  blocks it" diagnosis was wrong.
- [2026-08-02 — `importlib.reload` splits class identity](2026-08-02-importlib-reload-splits-class-identity.md)
  — order-dependent `Tools is Tools` failures (pre-existing on upstream/main);
  test the import-time expression's function directly instead of reloading.
