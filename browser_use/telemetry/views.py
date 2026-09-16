from abc import ABC, abstractmethod
from collections.abc import Sequence
from dataclasses import asdict, dataclass
from typing import Any, Literal

from browser_use.config import is_running_in_docker


@dataclass
class BaseTelemetryEvent(ABC):
	@property
	@abstractmethod
	def name(self) -> str:
		pass

	@property
	def properties(self) -> dict[str, Any]:
		props = {
			k: v
			for k, v in asdict(self).items()
			if k not in {'name', 'error_message', 'parent_process_cmdline', 'server_name', 'command'}
		}
		# Add Docker context if running in Docker
		props['is_docker'] = is_running_in_docker()
		return props


@dataclass
class AgentTelemetryEvent(BaseTelemetryEvent):
	# start details
	task: str
	model: str
	model_provider: str
	max_steps: int
	max_actions_per_step: int
	use_vision: bool | Literal['auto']
	version: str
	source: str
	cdp_url: str | None
	agent_type: str | None
	# step details
	action_errors: Sequence[str | None]
	action_history: Sequence[list[dict] | None]
	urls_visited: Sequence[str | None]
	# end details
	steps: int
	total_input_tokens: int
	total_output_tokens: int
	prompt_cached_tokens: int
	total_tokens: int
	total_duration_seconds: float
	success: bool | None
	final_result_response: str | None
	error_message: str | None
	# judge details
	judge_verdict: bool | None = None
	judge_reasoning: str | None = None
	judge_failure_reason: str | None = None
	judge_reached_captcha: bool | None = None
	judge_impossible_task: bool | None = None

	name: str = 'agent_event'

	@property
	def properties(self) -> dict[str, Any]:
		"""Return operational metrics without user prompts, URLs, actions, or results."""
		return {
			'model': self.model,
			'model_provider': self.model_provider,
			'max_steps': self.max_steps,
			'max_actions_per_step': self.max_actions_per_step,
			'use_vision': self.use_vision,
			'version': self.version,
			'source': self.source,
			'agent_type': self.agent_type,
			'steps': self.steps,
			'total_input_tokens': self.total_input_tokens,
			'total_output_tokens': self.total_output_tokens,
			'prompt_cached_tokens': self.prompt_cached_tokens,
			'total_tokens': self.total_tokens,
			'total_duration_seconds': self.total_duration_seconds,
			'success': self.success,
			'is_docker': is_running_in_docker(),
		}


@dataclass
class MCPClientTelemetryEvent(BaseTelemetryEvent):
	"""Telemetry event for MCP client usage"""

	server_name: str
	command: str
	tools_discovered: int
	version: str
	action: str  # 'connect', 'disconnect', 'tool_call'
	tool_name: str | None = None
	duration_seconds: float | None = None
	error_message: str | None = None

	name: str = 'mcp_client_event'


@dataclass
class MCPServerTelemetryEvent(BaseTelemetryEvent):
	"""Telemetry event for MCP server usage"""

	version: str
	action: str  # 'start', 'stop', 'tool_call'
	tool_name: str | None = None
	duration_seconds: float | None = None
	error_message: str | None = None
	parent_process_cmdline: str | None = None

	name: str = 'mcp_server_event'
