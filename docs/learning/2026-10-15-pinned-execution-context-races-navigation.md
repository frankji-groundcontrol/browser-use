# A pinned execution context races navigation, and load makes it lose

Date: 2026-10-15

- `Page::evaluate` in chromiumoxide 0.7 asks the target handler for a cached
  execution context and sends `Runtime.evaluate` with that `contextId`. While a
  navigation commits, the cached context is already destroyed, so Chrome answers
  `-32000 Cannot find context with specified id`. On an idle machine the window
  is too short to hit; under CPU load it is wide enough that
  `forbidden_first_page_and_mutations_fail_closed` failed 4 runs in 8.
  `state()` now reads the title with a raw `Page::execute(EvaluateParams)` and no
  `contextId`, so Chrome uses the main frame's current context: 10 of 10 runs
  pass under the same load.
- The bug only showed up once the test copied the failing shape: a
  script-initiated navigation (`location.href = …`) with `state()` polled while
  it commits. A `goto`-based stress test stayed green before the fix because
  `goto` waits for the commit. A reproduction that is green on the broken code
  is not evidence; keep it only if it fails for the cause you named.
- Chrome refuses script-initiated top-level navigation to a `data:` URL, so
  such a test times out for an unrelated reason. Serve the pages from a loopback
  HTTP server.
- Roughly fifteen other `self.page.evaluate(...)` calls in `bu-cdp` shared the
  latent race. They were first left alone on purpose: `Page::evaluate(&str)` also
  decides whether a string is a function or an expression, so replacing every
  call mechanically changes behaviour. On 2026-10-16 they were all moved to one
  `BrowserPage::eval` helper (raw `Runtime.evaluate`, no `contextId`,
  `awaitPromise`, `returnByValue`; a thrown script becomes an error with its
  description). Every script involved was already an expression, and the suite
  passed unchanged. The CPU-load navigation test now polls `state()`, `scroll_y()`
  and `content()`; putting `scroll_y` back on `Page::evaluate` makes it fail with
  `Cannot find context with specified id`. The other sites are covered only
  through the shared helper.
- The ten-minute "hang" was never reproduced as a true hang. One 75 s stall
  under ten CPU hogs had the test thread idle in `kevent` and Chrome's helpers
  runnable at about 0% CPU; 18 further hogged runs did not stall. Treat a silent
  run as starvation until a stack sample says otherwise, and bound the test with
  a wall-clock `tokio::time::timeout` so a stall fails and names itself instead
  of sitting silent. The policy test now has a 120 s bound.
- A password field whose `type` is flipped to `text` by a reveal toggle loses
  every static tell. Remembering, per tab, which backend node ids ever looked
  sensitive (cleared on `navigate()`) keeps it redacted after the flip. A stale
  id can only over-redact. The remaining gap is a field that was never seen as a
  password and has no password-like `name`, `id`, `placeholder`, `aria-label` or
  `autocomplete`.

Related: [review findings](../issues/2026-10-15-mbp-local-wip-review-findings.md) ·
[flaky live-chrome tests](../issues/2026-08-27-flaky-live-chrome-tests.md).
