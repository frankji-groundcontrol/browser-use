# Developer guide

## Setup

```bash
uv venv --python 3.11
source .venv/bin/activate
uv sync
```

## Quality gates

```bash
uv run pytest -vxs tests/ci      # CI test suite (real objects; only the LLM is faked)
uv run pyright                   # type check
uv run ruff check --fix          # lint
uv run ruff format               # format
uv run pre-commit run --all-files
```

Conventions (see [`CLAUDE.md`](../../../CLAUDE.md) for the full contract):

- Async Python ≥ 3.11, **tabs** for indentation.
- Modern typing (`str | None`, `list[str]`, `dict[str, Any]`).
- Main logic in `service.py`; pydantic models in `views.py`; events in
  `events.py`.
- Tests use real objects (never mock anything but the LLM); serve HTML with
  `pytest-httpserver`, never real remote URLs.

## Where things live

See [architecture/](../../architecture/index.md) for the subsystem map. Start
with `browser_use/browser/session.py` (the CDP-backed `BrowserSession`) and
`browser_use/mcp/server.py` (the MCP tool surface).
