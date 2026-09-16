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


@pytest.mark.asyncio
async def test_click_uses_geometry_after_scroll():
	from browser_use.actor.element import Element

	client = SimpleNamespace()
	client.send = SimpleNamespace(
		Page=SimpleNamespace(
			getLayoutMetrics=AsyncMock(return_value={'layoutViewport': {'clientWidth': 100, 'clientHeight': 100}})
		),
		DOM=SimpleNamespace(
			getContentQuads=AsyncMock(
				side_effect=[{'quads': [[200, 200, 220, 200, 220, 220, 200, 220]]}, {'quads': [[10, 20, 30, 20, 30, 40, 10, 40]]}]
			),
			scrollIntoViewIfNeeded=AsyncMock(),
		),
		Input=SimpleNamespace(dispatchMouseEvent=AsyncMock()),
	)
	element = Element(cast(Any, SimpleNamespace(cdp_client=client)), 7, 'sid')
	await element.click()
	move = client.send.Input.dispatchMouseEvent.await_args_list[0].kwargs['params']
	assert (move['x'], move['y']) == (20, 30)


@pytest.mark.asyncio
async def test_drag_moves_with_held_button_state():
	from browser_use.actor.element import Element

	client = SimpleNamespace(send=SimpleNamespace(Input=SimpleNamespace(dispatchMouseEvent=AsyncMock())))
	element = Element(cast(Any, SimpleNamespace(cdp_client=client)), 1, 'sid')
	await element.drag_to({'x': 100, 'y': 100}, source_position={'x': 0, 'y': 0})
	moves = [
		call.args[0]
		for call in client.send.Input.dispatchMouseEvent.await_args_list
		if call.args and call.args[0]['type'] == 'mouseMoved'
	]
	assert len(moves) == 10
	assert all(move['buttons'] == 1 for move in moves)
