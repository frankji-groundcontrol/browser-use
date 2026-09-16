"""Bounded subprocess JSON-RPC transport for the beta agent."""

from __future__ import annotations

import asyncio
import json
import os
from contextlib import suppress
from typing import Any

TERMINAL_INSTALL_COMMAND = 'curl -fsSL https://browser-use.com/terminal/install.sh | sh'


class BetaAgentError(RuntimeError):
	"""Raised when the beta agent cannot run a task."""


class RustSdkJsonRpcError(BetaAgentError):
	"""Raised when the Rust SDK server returns a JSON-RPC error."""

	def __init__(self, code: int, message: str) -> None:
		super().__init__(message)
		self.code = code
		self.message = message


class RustSdkClient:
	"""Minimal stdio JSON-RPC client for browser-use-terminal sdk-server."""

	def __init__(self, command: list[str], env: dict[str, str]) -> None:
		self.command = list(command)
		self.env = dict(env)
		self.process: asyncio.subprocess.Process | None = None
		self._reader_task: asyncio.Task[Any] | None = None
		self._stderr_task: asyncio.Task[Any] | None = None
		self._next_id = 1
		self._pending: dict[int, asyncio.Future[Any]] = {}
		self._write_lock = asyncio.Lock()
		self._lifecycle_lock = asyncio.Lock()
		self.stderr_lines: list[str] = []
		self.notifications: list[dict[str, Any]] = []
		self.notification_queue: asyncio.Queue[dict[str, Any]] = asyncio.Queue(maxsize=2000)
		self.stream_limit = int(os.environ.get('BROWSER_USE_SDK_STREAM_LIMIT_BYTES', str(64 * 1024 * 1024)))
		self.read_chunk_size = int(os.environ.get('BROWSER_USE_SDK_READ_CHUNK_BYTES', str(1024 * 1024)))
		self.max_line_bytes = int(os.environ.get('BROWSER_USE_SDK_MAX_LINE_BYTES', str(512 * 1024 * 1024)))

	async def start(self) -> None:
		async with self._lifecycle_lock:
			if self.process is not None:
				if self.process.returncode is None and (self._reader_task is None or not self._reader_task.done()):
					return
				await self._close()
			try:
				self.process = await asyncio.create_subprocess_exec(
					*self.command,
					stdin=asyncio.subprocess.PIPE,
					stdout=asyncio.subprocess.PIPE,
					stderr=asyncio.subprocess.PIPE,
					env=self.env,
					limit=self.stream_limit,
				)
			except (FileNotFoundError, PermissionError) as exc:
				command = self.command[0] if self.command else 'browser-use-terminal'
				raise BetaAgentError(
					f'Could not start Rust SDK server command {command!r}. '
					f'Install Browser Use Terminal with `{TERMINAL_INSTALL_COMMAND}`, '
					'or set BROWSER_USE_TERMINAL_BINARY to a built terminal CLI.'
				) from exc
			self._reader_task = asyncio.create_task(self._read_stdout())
			self._stderr_task = asyncio.create_task(self._read_stderr())

	async def call(self, method: str, params: dict[str, Any] | None = None) -> Any:
		await self.start()
		loop = asyncio.get_running_loop()
		async with self._write_lock:
			process = self.process
			if process is None or process.stdin is None:
				raise BetaAgentError('Rust SDK server stdin is unavailable')
			request_id = self._next_id
			self._next_id += 1
			future: asyncio.Future[Any] = loop.create_future()
			self._pending[request_id] = future
			request = {
				'jsonrpc': '2.0',
				'id': request_id,
				'method': method,
				'params': params or {},
			}
			try:
				process.stdin.write((json.dumps(request) + '\n').encode('utf-8'))
				await process.stdin.drain()
			except BaseException as exc:
				self._pending.pop(request_id, None)
				if future.done() and not future.cancelled():
					future.exception()
				else:
					future.cancel()
				if isinstance(exc, (BrokenPipeError, ConnectionResetError)):
					raise BetaAgentError(f'Rust SDK server pipe closed while sending {method}') from exc
				raise

		try:
			return await future
		except asyncio.CancelledError:
			self._pending.pop(request_id, None)
			raise

	async def close(self) -> None:
		# Finish reaping even if the caller is cancelled while closing.
		async def cleanup() -> None:
			async with self._lifecycle_lock:
				await self._close()

		task = asyncio.create_task(cleanup())
		try:
			await asyncio.shield(task)
		except asyncio.CancelledError:
			await task
			raise

	async def _close(self) -> None:
		process = self.process
		if process is None:
			return
		if process.stdin is not None:
			process.stdin.close()
		if process.returncode is None:
			with suppress(ProcessLookupError):
				process.terminate()
			try:
				await asyncio.wait_for(process.wait(), timeout=2)
			except TimeoutError:
				with suppress(ProcessLookupError):
					process.kill()
				await process.wait()
		for task in (self._reader_task, self._stderr_task):
			if task is not None:
				task.cancel()
		await asyncio.gather(
			*(task for task in (self._reader_task, self._stderr_task) if task is not None and task is not asyncio.current_task()),
			return_exceptions=True,
		)
		self._fail_all(BetaAgentError('Rust SDK server closed'))
		self.process = None
		self._reader_task = None
		self._stderr_task = None

	async def _read_stdout(self) -> None:
		assert self.process is not None
		assert self.process.stdout is not None
		buffer = bytearray()
		try:
			while True:
				chunk = await self.process.stdout.read(self.read_chunk_size)
				if not chunk:
					if buffer and not self._handle_stdout_line(bytes(buffer)):
						return
					return
				buffer.extend(chunk)
				while True:
					newline_index = buffer.find(b'\n')
					if newline_index < 0:
						break
					raw_line = bytes(buffer[:newline_index])
					del buffer[: newline_index + 1]
					if not self._handle_stdout_line(raw_line):
						return
				if len(buffer) > self.max_line_bytes:
					self._fail_all(BetaAgentError(f'Rust SDK JSON-RPC line exceeded {self.max_line_bytes} bytes without newline'))
					return
		except Exception as exc:
			message = f'Rust SDK stdout reader failed: {exc}'
			self.stderr_lines.append(message)
			self._fail_all(BetaAgentError(message))
		finally:
			if self._pending:
				detail = '\n'.join(self.stderr_lines[-20:])
				message = 'Rust SDK server exited before responding'
				if detail:
					message = f'{message}: {detail}'
				self._fail_all(BetaAgentError(message))

	def _handle_stdout_line(self, raw_line: bytes) -> bool:
		if len(raw_line) > self.max_line_bytes:
			self._fail_all(BetaAgentError(f'Rust SDK JSON-RPC line exceeded {self.max_line_bytes} bytes'))
			return False
		try:
			line = raw_line.decode('utf-8').strip()
		except UnicodeDecodeError:
			self._fail_all(BetaAgentError('Invalid Rust SDK UTF-8 frame'))
			return False
		if not line:
			return True
		try:
			message = json.loads(line)
		except json.JSONDecodeError:
			self._fail_all(BetaAgentError('Invalid Rust SDK JSON-RPC line'))
			return False
		self._handle_message(message)
		return True

	async def _read_stderr(self) -> None:
		assert self.process is not None
		assert self.process.stderr is not None
		while chunk := await self.process.stderr.read(64 * 1024):
			self.stderr_lines.extend(line[:4096] for line in chunk.decode('utf-8', errors='replace').splitlines())
			del self.stderr_lines[:-500]

	def _handle_message(self, message: Any) -> None:
		if not isinstance(message, dict):
			self._fail_all(BetaAgentError('Rust SDK server emitted non-object JSON-RPC message'))
			return
		method = message.get('method')
		if method in {'agent.event', 'agent.projected_event'}:
			notification = {
				'method': method,
				'params': message.get('params') if isinstance(message.get('params'), dict) else {},
			}
			self.notifications.append(notification)
			del self.notifications[:-2000]
			self.notification_queue.put_nowait(notification)
			return
		if 'id' not in message:
			return
		request_id = message.get('id')
		if type(request_id) is not int:
			self._fail_all(BetaAgentError('Rust SDK JSON-RPC response id must be an integer'))
			return
		# Validate before removing the future: a malformed error must not strand
		# the request outside the pending table when the reader fails.
		if ('result' in message) == ('error' in message):
			self._fail_all(BetaAgentError('Rust SDK response requires exactly one result or error'))
			return
		error = message.get('error')
		if 'error' in message and (
			not isinstance(error, dict) or type(error.get('code')) is not int or not isinstance(error.get('message'), str)
		):
			self._fail_all(BetaAgentError('Invalid Rust SDK JSON-RPC error'))
			return
		future = self._pending.pop(request_id, None)
		if future is None or future.done():
			return
		if isinstance(error, dict):
			future.set_exception(RustSdkJsonRpcError(error['code'], error['message']))
		else:
			future.set_result(message['result'])

	def _fail_all(self, error: BaseException) -> None:
		for future in self._pending.values():
			if not future.done():
				future.set_exception(error)
		self._pending.clear()
