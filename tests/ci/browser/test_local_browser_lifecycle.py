"""Real-process proofs for local Chrome ownership and launch cleanup."""

import asyncio

import psutil
import pytest

from browser_use.browser.profile import BrowserProfile
from browser_use.browser.session import BrowserSession
from browser_use.browser.watchdogs.local_browser_watchdog import LocalBrowserWatchdog


async def test_keep_alive_stop_reconnects_to_same_process_and_kill_reaps():
	session = BrowserSession(browser_profile=BrowserProfile(headless=True, user_data_dir=None, keep_alive=True))
	try:
		await session.start()
		watchdog = session._local_browser_watchdog
		assert watchdog is not None and watchdog._subprocess is not None
		process = watchdog._subprocess
		await session.stop()
		assert process.is_running()
		await session.start()
		assert session._local_browser_watchdog is watchdog
		assert watchdog._subprocess is process
		assert (await session.get_browser_state_summary(include_screenshot=False)).url
	finally:
		await session.kill()
	assert not process.is_running()


@pytest.mark.parametrize('cancel', [False, True])
async def test_failed_or_cancelled_readiness_reaps_real_child(monkeypatch, cancel):
	"""Inject failure only at the readiness boundary; launch and reap actual Chromium."""
	session = BrowserSession(browser_profile=BrowserProfile(headless=True, user_data_dir=None))
	watchdog = LocalBrowserWatchdog(browser_session=session, event_bus=session.event_bus)
	spawn = asyncio.create_subprocess_exec
	spawned = []
	ready = asyncio.Event()

	async def record_spawn(*args, **kwargs):
		assert kwargs['stdout'] == asyncio.subprocess.DEVNULL
		assert kwargs['stderr'] == asyncio.subprocess.DEVNULL
		process = await spawn(*args, **kwargs)
		spawned.append(process)
		return process

	async def fail_readiness(_port):
		ready.set()
		if cancel:
			await asyncio.Future()
		raise TimeoutError('injected CDP readiness failure')

	monkeypatch.setattr(asyncio, 'create_subprocess_exec', record_spawn)
	monkeypatch.setattr(LocalBrowserWatchdog, '_wait_for_cdp_url', staticmethod(fail_readiness))
	task = asyncio.create_task(watchdog._launch_browser())
	try:
		await asyncio.wait_for(ready.wait(), timeout=10)
		if cancel:
			task.cancel()
		with pytest.raises(asyncio.CancelledError if cancel else TimeoutError):
			await asyncio.wait_for(task, timeout=10)
		assert len(spawned) == 1
		assert spawned[0].returncode is not None
		assert not psutil.pid_exists(spawned[0].pid)
	finally:
		for process in spawned:
			if process.returncode is None:
				process.kill()
			await process.wait()
