from __future__ import annotations

import asyncio
import sys
import time
from types import SimpleNamespace
from typing import Any, cast
from unittest.mock import AsyncMock

import pytest


@pytest.mark.asyncio
async def test_python_agent_awaits_async_callable_step_callback():
	from browser_use.agent.service import Agent

	called = asyncio.Event()

	class AsyncCallback:
		async def __call__(self, *_args):
			called.set()

	agent = cast(Any, Agent.__new__(Agent))
	agent.register_new_step_callback = AsyncCallback()
	agent.state = SimpleNamespace(last_model_output=object(), n_steps=1)
	agent.settings = SimpleNamespace(save_conversation_path=None)

	await agent._handle_post_llm_processing(None, [])
	assert called.is_set()


@pytest.mark.asyncio
async def test_mcp_client_reconnect_clears_disconnect_event():
	from browser_use.mcp.client import MCPClient

	client = MCPClient(server_name='test', command='test')

	async def fake_stdio(server_params):
		_ = server_params
		client._connected = True
		await client._disconnect_event.wait()

	cast(Any, client)._run_stdio_client = fake_stdio
	await client.connect()
	assert client._connected
	await client.disconnect()
	assert not client._connected

	await client.connect()
	assert client._connected
	await client.disconnect()


@pytest.mark.asyncio
async def test_mcp_client_cancelled_connect_stops_background_task():
	from browser_use.mcp.client import MCPClient

	client = MCPClient(server_name='test', command='test')
	started = asyncio.Event()

	async def hanging_stdio(server_params):
		_ = server_params
		started.set()
		await asyncio.sleep(60)

	cast(Any, client)._run_stdio_client = hanging_stdio
	connect = asyncio.create_task(client.connect())
	await started.wait()
	connect.cancel()
	with pytest.raises(asyncio.CancelledError):
		await connect
	assert client._stdio_task is not None and client._stdio_task.done()


@pytest.mark.asyncio
async def test_beta_run_enforces_call_timeout(monkeypatch):
	from browser_use.beta import Agent

	class LLM:
		model = 'test'

	class HangingSdk:
		stderr_lines = []
		notifications = []

		async def call(self, _method, _params):
			await asyncio.sleep(60)

	agent = Agent(task='test', llm=cast(Any, LLM()), directly_open_url=False)
	agent._initialize_run_lifecycle_state()

	async def ensure_sdk():
		return HangingSdk()

	monkeypatch.setattr(agent, '_ensure_sdk_client', ensure_sdk)
	history = await agent._run_sdk_agent(
		task='test', max_steps=1, started=time.time(), on_step_end=None, source='run', step_timeout=1
	)
	assert any('TimeoutError' in (error or '') for error in history.errors())


@pytest.mark.asyncio
async def test_beta_cleanup_bounds_unresponsive_close_rpc(monkeypatch):
	import browser_use.beta.service as beta_service
	from browser_use.beta import Agent

	class LLM:
		model = 'test'

	class HangingSdk:
		async def call(self, _method, _params):
			await asyncio.sleep(60)

	agent = Agent(task='test', llm=cast(Any, LLM()), directly_open_url=False)
	cast(Any, agent)._sdk_client = HangingSdk()
	agent._sdk_agent_id = 'test-agent'
	agent._sdk_browser_id = 'test-browser'
	original_wait_for = asyncio.wait_for

	async def fast_wait_for(awaitable, timeout):
		return await original_wait_for(awaitable, min(timeout, 0.01))

	monkeypatch.setattr(beta_service.asyncio, 'wait_for', fast_wait_for)
	await agent._close_sdk_browser_resources()
	assert agent._sdk_agent_id is None
	assert agent._sdk_browser_id is None


@pytest.mark.asyncio
async def test_sdk_rejects_oversized_terminated_line():
	from browser_use.beta.service import BetaAgentError, RustSdkClient

	script = 'import sys; sys.stdin.readline(); print("x" * 200, flush=True)'
	client = RustSdkClient([sys.executable, '-c', script], {})
	client.max_line_bytes = 128
	try:
		with pytest.raises(BetaAgentError, match='line exceeded 128 bytes'):
			await client.call('test')
	finally:
		await client.close()


@pytest.mark.asyncio
async def test_sdk_restarts_after_stdout_reader_failure():
	from browser_use.beta.service import BetaAgentError, RustSdkClient

	script = 'import sys,time; sys.stdin.readline(); print("invalid", flush=True); time.sleep(60)'
	client = RustSdkClient([sys.executable, '-c', script], {})
	try:
		for _ in range(2):
			with pytest.raises(BetaAgentError, match='Invalid Rust SDK JSON-RPC line'):
				await asyncio.wait_for(client.call('test'), timeout=1)
	finally:
		await client.close()


@pytest.mark.asyncio
async def test_crash_watchdog_attaches_to_created_target():
	from browser_use.browser.events import TabCreatedEvent
	from browser_use.browser.watchdogs.crash_watchdog import CrashWatchdog

	watchdog = CrashWatchdog.model_construct(
		browser_session=cast(Any, SimpleNamespace(logger=SimpleNamespace())), event_bus=cast(Any, SimpleNamespace())
	)
	attach = AsyncMock()
	object.__setattr__(watchdog, 'attach_to_target', attach)
	await watchdog.on_TabCreatedEvent(TabCreatedEvent(target_id='new-target', url='about:blank'))
	attach.assert_awaited_once_with('new-target')
