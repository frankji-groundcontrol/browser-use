import asyncio
from types import SimpleNamespace
from typing import Any, cast
from unittest.mock import AsyncMock

import pytest


@pytest.mark.asyncio
async def test_page_attaches_once_when_session_is_requested_concurrently():
	from browser_use.actor.page import Page

	client = SimpleNamespace()
	client.send = SimpleNamespace()
	client.send.Target = SimpleNamespace(attachToTarget=AsyncMock(return_value={'sessionId': 'sid'}))
	for domain in ('Page', 'DOM', 'Runtime', 'Network'):
		setattr(client.send, domain, SimpleNamespace(enable=AsyncMock()))
	session = SimpleNamespace(cdp_client=client)
	page = Page(cast(Any, session), 'target')

	assert await asyncio.gather(page.session_id, page.session_id) == ['sid', 'sid']
	client.send.Target.attachToTarget.assert_awaited_once()


@pytest.mark.asyncio
async def test_mouse_down_uses_last_moved_position():
	from browser_use.actor.mouse import Mouse

	client = SimpleNamespace()
	client.send = SimpleNamespace(Input=SimpleNamespace(dispatchMouseEvent=AsyncMock()))
	m = Mouse(cast(Any, SimpleNamespace(cdp_client=client)), session_id='sid')
	await m.move(23, 41)
	await m.down()

	assert client.send.Input.dispatchMouseEvent.await_args_list[-1].args[0]['x'] == 23
	assert client.send.Input.dispatchMouseEvent.await_args_list[-1].args[0]['y'] == 41


@pytest.mark.asyncio
async def test_detached_element_reports_actionable_error():
	from browser_use.actor.element import Element

	client = SimpleNamespace()
	client.send = SimpleNamespace(DOM=SimpleNamespace(pushNodesByBackendIdsToFrontend=AsyncMock(return_value={'nodeIds': []})))
	element = Element(cast(Any, SimpleNamespace(cdp_client=client)), 7, 'sid')

	with pytest.raises(RuntimeError, match='detached'):
		await element._get_node_id()
