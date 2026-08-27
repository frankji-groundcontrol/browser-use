# Python code style and change workflow

Date: 2026-08-26 · Trigger: any change to the Python library under `browser_use/`

Extracted from `CLAUDE.md` so the router stays thin. This is the contract for
writing and changing Python code in this repo.

## Code style

- Async Python throughout.
- **Tabs** for indentation in all Python code, not spaces.
- Modern typing (Python >3.12 style): `str | None` over `Optional[str]`,
  `list[str]` over `List[str]`, `dict[str, Any]` over `Dict[str, Any]`.
- Keep console logging in separate methods prefixed `_log_...` — for example
  `def _log_pretty_path(path: Path) -> str` — so it does not clutter main logic.
- Use pydantic v2 models for internal data and for any user-facing API parameter
  that would otherwise be a loose dict.
- Tune model behaviour with
  `model_config = ConfigDict(extra='forbid', validate_by_name=True, validate_by_alias=True, ...)`,
  and push validation into `Annotated[..., AfterValidator(...)]` rather than
  helper methods on the model.
- Main logic per sub-component goes in `service.py`; pydantic models in
  `views.py` unless one grows big enough to deserve its own file; event types in
  `events.py`.
- Use runtime assertions at the start and end of functions to enforce
  constraints and assumptions.
- New id fields: `from uuid_extensions import uuid7str` plus
  `id: str = Field(default_factory=uuid7str)`.

## Hard constraints

- **Always use `uv`**, never `pip`.
- **Use real model names.** Do not "correct" `gpt-4o` to `gpt-4`; they are
  distinct models, and users try models you have not heard of.
- **Never create example files** to show off a feature — it just makes the tree
  messy. Test inline in the terminal instead.
- Use descriptive names and docstrings for every action.
- Return `ActionResult` with structured content so the agent can reason better.
- Run pre-commit before opening a PR.

## Working with CDP

CDP goes through [`cdp-use`](https://github.com/browser-use/cdp-use), a thin
typed wrapper over the websocket calls. All CDP client and session management —
and every other CDP helper — stays in `browser_use/browser/session.py`.

```python
cdp_client.send.DOMSnapshot.enable(session_id=session_id)

# Prefer the typed params object over a bare dict:
from cdp_use.cdp.target import ActivateTargetParameters
cdp_client.send.Target.attachToTarget(params=ActivateTargetParameters(targetId=target_id, flatten=True))

# Event registration — note: cdp_client.on(...) does NOT exist.
cdp_client.register.Browser.downloadWillBegin(callback_func_here)
```

See [architecture 01 — CDP transport](../architecture/01-cdp-transport-and-session-manager.md).

## Tests

- Never mock anything — use real objects. The **only** exception is the LLM,
  which has pytest fixtures and helpers in `conftest.py`.
- Never use real remote URLs (`https://google.com`, `https://example.com`). Set
  up `pytest-httpserver` in a fixture serving the HTML the test needs.
- Once a test file passes, move it into `tests/ci/`, which CI discovers and runs
  on every commit. Event-specific tests belong in
  `tests/ci/test_action_EventNameHere.py`.
- Modern pytest-asyncio: no `@pytest.mark.asyncio` decorator needed, just async
  functions. Use `loop = asyncio.get_event_loop()` inside a test rather than
  taking `event_loop` as an argument, and give fixtures a bare `@pytest.fixture`.

## Changing existing behaviour

1. Find or write tests that pin the **existing** design, and confirm they pass
   before you change anything.
2. Write failing tests for the new design; run them and confirm they fail for
   the expected reason.
3. Implement the change, adding tests during development wherever an assumption
   needs checking.
4. Run the full `tests/ci` suite. Confirm the new design works **and** that
   backward compatibility did not break.
5. Condense and deduplicate the test logic into one file, then re-read it to
   make sure the same thing is not asserted repeatedly. Scan `tests/` for other
   files that need updating or condensing.
6. Update `docs/` and `examples/` so they match the implementation and tests.

For truly massive refactors, lean on simple event buses and job queues to break
the system into smaller services that each own an isolated piece of state.

## Editing mechanics

If an in-place edit will not apply, shorten the match string to one or two lines
instead of three. If that still fails, insert the new code as new lines, then
delete the old code in a second step rather than replacing in one shot.

## Keep examples and tests current

Read the relevant files in `examples/` and `tests/` (especially `tests/ci/*.py`)
for context before changing behaviour, and update them in the same change.

Related: [developer guide](../usage/developers/README.md),
[running the Python CI suite](running-python-ci.md).
