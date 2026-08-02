# `importlib.reload` splits class identity → order-dependent test failures

Date: 2026-08-02 · Scope: Python `tests/ci` (present on clean upstream/main too)

## Problem

Full-suite runs failed 2 `test_beta_agent.py` tests with the absurd-looking

```
AssertionError: assert <class 'browser_use.tools.service.Tools'>
                    is <class 'browser_use.tools.service.Tools'>
```

Both tests passed in isolation. Reproduced identically on clean
`upstream/main` (same counts), so this predates the fork's changes.

## Root cause

`test_action_timeout.py` called `importlib.reload(browser_use.tools.service)`
to test `BROWSER_USE_ACTION_TIMEOUT_S` parsing. Each reload **re-executes the
module and mints a new `Tools` class object**, while modules that already did
`from browser_use.tools.service import Tools` (e.g. `agent.service`) keep the
original. Every later `is` / `isinstance` check then compares two `Tools`
classes that print identically and fail. The teardown fixture's
"restore" reload polluted even when the test passed. With `-p no:randomly`,
`test_action_timeout.py` sorts right before `test_beta_agent.py`; the two
files alone reproduce it in 2 seconds.

## Resolution

The module-level line under test is a pure call:
`_DEFAULT_ACTION_TIMEOUT_S = _parse_env_action_timeout(os.getenv(...))`.
The test now calls `_parse_env_action_timeout` directly with the same bad
values — the exact import-time path, zero reloads — and the reload fixture is
gone (commit `1bc922a91`).

## Lesson

`importlib.reload` in a test is a suite-wide contaminant whenever *any* other
module holds a `from X import SomeClass` reference. If the behavior under test
is a module-level expression, test the expression's function directly instead
of re-importing the world.
