# Reproduce on the object production uses, and assert the precondition

Date: 2026-10-16

- The Flutter index-typing bug did not reproduce in a `bu-cdp` harness that opened
  its page with `new_page()`. Production adopts Chromium's *initial* tab, which
  reports `document.hasFocus() === false`; a new tab reports `true`. The harness
  and the product differed in exactly the state that mattered. Before trusting a
  green reproduction attempt, check that it acquires the page the same way the
  product does.
- A workaround that makes a symptom vanish can fit more than one wrong
  explanation. Tapping the field first worked because a real mouse press focuses
  the page, not because Flutter needed a pointer event. What separated the
  hypotheses was one state variable varied over a matrix (new tab, initial tab,
  initial tab plus `bringToFront`, initial tab plus focus emulation) on the real
  app, plus the orderings of a first and second fill.
- Two regression tests I wrote asserted that a `focus` handler had run. The
  handler ran by the time the assertion read it, with or without the fix, so both
  stayed green with the fix mutated away. A test of a lazily arriving effect has
  to assert the precondition (`document.hasFocus()` before any typing) or the
  order of events, not the eventual state. Run the mutation before believing the
  test; this one was caught only because the mutation was run.
- A mechanism claimed from knowledge ("Blink delivers no `focus` event to an
  unfocused page") was contradicted by a probe in the same session: the handler
  fired before the `input` event. Record what was measured and mark the rest as
  not traced.
- A test that reaches the code under test through a handle the setup already
  prepared proves nothing about that code. `a_tab_switched_back_to_is_focused`
  went through `primary_page()`, which had already enabled emulation for that
  tab, so reverting `switch_tab` left it green; the adversarial review found
  it, not the mutation I ran. The replacement uses a second session that never
  touched the tab. Also, a field that sounds like the discriminator is not
  always one: `Browser.getVersion` says `Chrome/…` for headless, and only the
  user agent says `HeadlessChrome`. The first predicate keyed on the product
  string and would have silently disabled the fix for every attached headless
  browser; the new test failed on the first run, which is how it showed.
- A mutation helper that asserts its pattern matches exactly once is worth its
  weight. Two mutation runs here applied nothing (a pattern that rustfmt had
  reflowed, and a wrong expected count) and reported green until the assertion
  traceback was read.

Scope: any fix whose test drives a browser. Related:
[a pinned execution context races navigation](2026-10-15-pinned-execution-context-races-navigation.md) ·
[Learning index](README.md)
