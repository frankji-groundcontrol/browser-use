# Running the Python CI suite on this machine

This machine has **no direct egress** — all traffic rides the local proxy.
Getting `tests/ci` green depends on the env, not just the code.

## The command

```bash
env -u ALL_PROXY -u all_proxy \
  uv run pytest tests/ci -q -p no:randomly --timeout=180 --timeout-method=thread
```

- **Unset only the SOCKS vars** (`ALL_PROXY`/`all_proxy`): httpx refuses SOCKS
  without the `socksio` extra, which trips one LLM test. **Keep
  `HTTPS_PROXY`/`HTTP_PROXY`** — stripping everything removes the only network
  path and outbound `connect()`s hang for minutes
  ([issue record](../issues/2026-08-02-tests-ci-wedge-proxy-and-extension-download.md)).
- **`--timeout-method=thread`** is the diagnostic workhorse: on a hang it dumps
  the stuck test's Python stack instead of silently killing the run. That dump
  is how the unbounded extension download was found.
- `-p no:randomly` gives a deterministic order — required to reproduce (and
  bisect) order-dependent pollution like the
  [`importlib.reload` class split](../issues/2026-08-02-importlib-reload-splits-class-identity.md).

Healthy baseline (2026-08-02, browser-use 0.13.7 merge): **1024 passed,
34 skipped, 0 failed** in ~8 minutes.

## Triage rules learned here

- A test that fails in the full run but passes alone (`pytest <file> -k <name>`)
  is order pollution — bisect by running file *pairs*, starting from files that
  `grep -l "importlib\|sys.modules" tests/ci/` flags.
- Before blaming the sandbox, prove the network claim:
  `uv run python -c "import urllib.request; ..."` with and without the proxy
  vars. The "sandbox blocks it" diagnosis was wrong once already.
- To check whether a failure is the fork's or upstream's:
  `git worktree add /tmp/bu-upstream upstream/main` and run the same command
  there — identical counts settle it in one run.
