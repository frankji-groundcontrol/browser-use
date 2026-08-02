# `tests/ci` wedged for 25+ minutes: two stacked root causes

Date: 2026-08-02 · Scope: running the Python CI suite on this machine

## Problem

`uv run pytest tests/ci` sat for 25 minutes with ~5 seconds of CPU and no
output. It looked like "the suite needs network/browser resources the sandbox
blocks" — that diagnosis was wrong, and unpacking it found one environment
fact and one real upstream bug.

## Evidence and root causes

Diagnosed with `--timeout=60 --timeout-method=thread`, which dumps the stack of
a hung test instead of just killing the run. The dump ended mid
`sock.connect()` inside `profile.py::_download_extension`.

1. **Environment: this machine has no direct egress.** All traffic goes
   through the local proxy (`HTTPS_PROXY=http://127.0.0.1:7897`). Stripping
   *all* proxy vars (the reflex when `ALL_PROXY` caused
   `socksio`-not-installed errors in httpx) removes the only network path, so
   any outbound `connect()` hangs until the OS gives up. Only the SOCKS vars
   (`ALL_PROXY`/`all_proxy`) need unsetting; `HTTPS_PROXY` must stay.
2. **Upstream bug: unbounded extension download during browser launch.**
   `BrowserProfile._download_extension` called `urllib.request.urlopen(url)`
   with **no timeout**, inside `get_args()` → browser launch. A stalled
   network therefore wedged the launch itself — and with it every test that
   starts a browser.

## Resolution

- Fixed the code: 15s cap via `EXTENSION_DOWNLOAD_TIMEOUT_SECONDS` (commit
  `1a5e15d71`); the caller already degrades to launching without extensions.
  Regression test uses a local server that accepts and never answers.
- Recorded the runnable environment recipe in
  [practices/running-python-ci.md](../practices/running-python-ci.md).

With both in place the full suite finishes in ~8 minutes:
**1024 passed, 34 skipped, 0 failed**.
