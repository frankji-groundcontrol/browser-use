#!/usr/bin/env python3
"""browser-use MCP launcher for OpenAI-compatible gateways that reject the SDK.

Some OpenAI-compatible gateways (notably ChatGPT-account reverse proxies) sit
behind a WAF that 403-blocks any request whose ``User-Agent`` identifies the
official ``openai`` Python SDK -- which is exactly what browser-use uses
internally for its two LLM-backed MCP tools (``browser_extract_content`` and
``retry_with_browser_use_agent``). The MCP server exposes no header hook, so this
launcher patches ``openai.AsyncOpenAI`` at import time to send a neutral
User-Agent, then runs the standard ``browser-use --mcp`` server unchanged.

It can also normalize ``OPENAI_BASE_URL`` to include the ``/v1`` suffix the SDK
expects, and defaults to headless (useful on servers). Everything is env-driven;
nothing gateway-specific is hardcoded, so this file is safe to commit publicly.

Only the two LLM-backed tools need any of this. The 14 low-level browser tools
(navigate/click/type/get_state/get_html/screenshot/scroll/tabs) work with no key
and no gateway at all.

Environment variables
---------------------
OPENAI_API_KEY              Required for the 2 LLM tools; the 14 low-level tools need none.
OPENAI_BASE_URL             OpenAI-compatible endpoint. Include ``/v1`` yourself, or set
                            BROWSER_USE_MCP_FORCE_V1=1 to have this launcher append it.
BROWSER_USE_MCP_FORCE_V1    If truthy, append ``/v1`` to OPENAI_BASE_URL when it is missing.
BROWSER_USE_MCP_USER_AGENT  Override the neutral User-Agent (default: ``browser-use-mcp``).
BROWSER_USE_LLM_MODEL       Model id the gateway actually serves.
BROWSER_USE_HEADLESS        Defaults to ``true`` here (override with ``false`` for headful).

Usage
-----
Point your MCP client's launch command at this file, run with an interpreter that
has ``browser-use`` installed::

    /path/to/python /path/to/contrib/mcp/mcp-launch.py

See docs/usage/tools/mcp-multi-agent-setup.md for a full walkthrough.
"""

import os
import sys


def _truthy(value: str | None) -> bool:
	return str(value).strip().lower() in ("1", "true", "yes", "on")


# 1) Optional: append /v1 when the gateway serves its API there (opt-in).
_base = os.environ.get("OPENAI_BASE_URL", "").strip().rstrip("/")
if _base and _truthy(os.environ.get("BROWSER_USE_MCP_FORCE_V1")) and not _base.endswith("/v1"):
	os.environ["OPENAI_BASE_URL"] = _base + "/v1"

# 2) Server-friendly defaults (only applied if the caller left them unset).
os.environ.setdefault("BROWSER_USE_HEADLESS", "true")
os.environ.setdefault("ANONYMIZED_TELEMETRY", "false")

# 3) Force a neutral User-Agent so SDK-fingerprint-blocking gateways stop 403ing.
#    default_headers overrides the openai SDK's built-in "OpenAI/Python ..." UA.
import openai

_user_agent = os.environ.get("BROWSER_USE_MCP_USER_AGENT", "browser-use-mcp")
_orig_async_init = openai.AsyncOpenAI.__init__


def _async_init_with_ua(self, *args, **kwargs):
	headers = dict(kwargs.get("default_headers") or {})
	headers["User-Agent"] = _user_agent
	kwargs["default_headers"] = headers
	return _orig_async_init(self, *args, **kwargs)


openai.AsyncOpenAI.__init__ = _async_init_with_ua

# 4) Run the standard browser-use MCP server (identical to `browser-use --mcp`).
from browser_use.cli import main

if "--mcp" not in sys.argv:
	sys.argv = [sys.argv[0], "--mcp"]

main()
