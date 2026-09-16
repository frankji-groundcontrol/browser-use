"""Real temporary subprocess proofs for beta JSON-RPC lifecycle and framing."""

import asyncio
import sys

import pytest

from browser_use.beta.service import BetaAgentError, RustSdkClient


@pytest.mark.asyncio
async def test_concurrent_calls_share_one_child_and_close_reaps_it():
	client = RustSdkClient(
		[
			sys.executable,
			'-c',
			"""
import json, os, sys
for line in sys.stdin:
    request = json.loads(line)
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': os.getpid()}), flush=True)
""",
		],
		{},
	)
	try:
		pids = await asyncio.wait_for(asyncio.gather(*(client.call('pid') for _ in range(8))), 2)
		assert len(set(pids)) == 1
		process = client.process
	finally:
		await client.close()
	assert process is not None and process.returncode is not None
	assert not client._pending


@pytest.mark.asyncio
@pytest.mark.parametrize(
	'response',
	[
		b'{"jsonrpc":"2.0","id":1,"error":{"code":"invalid","message":"no"}}\n',
		b'{"jsonrpc":"2.0","id":true,"result":"wrong id"}\n',
		b'{"jsonrpc":"2.0","id":1,"result":"\xff"}\n',
		b'{"jsonrpc":"2.0","id":1}\n',
	],
)
async def test_malformed_rpc_fails_pending_call_promptly(response):
	client = RustSdkClient(
		[sys.executable, '-c', f'import sys; sys.stdin.readline(); sys.stdout.buffer.write({response!r}); sys.stdout.flush()'], {}
	)
	try:
		with pytest.raises(BetaAgentError):
			await asyncio.wait_for(client.call('test'), 1)
	finally:
		await client.close()


@pytest.mark.asyncio
async def test_stderr_without_newlines_is_drained_and_cancelled_call_is_removed():
	client = RustSdkClient(
		[
			sys.executable,
			'-c',
			"""
import json, sys, time
for line in sys.stdin:
    request = json.loads(line)
    sys.stderr.write('x' * (256 * 1024)); sys.stderr.flush()
    if request['method'] == 'wait':
        time.sleep(0.1)
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': 'ok'}), flush=True)
""",
		],
		{},
	)
	client.stream_limit = 1024
	try:
		with pytest.raises(TimeoutError):
			await asyncio.wait_for(client.call('wait'), 0.05)
		assert not client._pending
		assert await asyncio.wait_for(client.call('next'), 1) == 'ok'
		process = client.process
	finally:
		await client.close()
	assert process is not None and process.returncode is not None
	assert len(client.stderr_lines) <= 500


@pytest.mark.asyncio
async def test_child_death_fails_all_pending_requests():
	client = RustSdkClient([sys.executable, '-c', 'import sys; sys.stdin.readline(); sys.exit(17)'], {})
	try:
		with pytest.raises(BetaAgentError, match='exited before responding'):
			await asyncio.wait_for(client.call('die'), 1)
		process = client.process
	finally:
		await client.close()
	assert process is not None and process.returncode == 17


@pytest.mark.asyncio
async def test_close_reaps_child_with_full_stdin_even_when_cancelled():
	client = RustSdkClient(
		[sys.executable, '-c', 'import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)'], {}
	)
	await client.start()
	process = client.process
	pending = asyncio.create_task(client.call('blocked-write', {'data': 'x' * (2 * 1024 * 1024)}))
	await asyncio.sleep(0.1)
	closing = asyncio.create_task(client.close())
	await asyncio.sleep(0.05)
	closing.cancel()
	with pytest.raises(asyncio.CancelledError):
		await asyncio.wait_for(closing, 3)
	assert process is not None and process.returncode is not None
	assert not client._pending
	await asyncio.gather(pending, return_exceptions=True)
