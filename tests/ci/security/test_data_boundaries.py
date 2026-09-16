"""Regression checks for URL, configuration, telemetry, and cookie boundaries."""

from types import SimpleNamespace

from bubus import EventBus

from browser_use.browser import BrowserProfile, BrowserSession
from browser_use.browser.watchdogs.security_watchdog import SecurityWatchdog
from browser_use.config import CONFIG, load_and_migrate_config
from browser_use.skills.service import SkillService
from browser_use.telemetry.views import AgentTelemetryEvent, MCPServerTelemetryEvent


def test_full_url_allowlist_requires_exact_authority():
	profile = BrowserProfile(allowed_domains=['https://example.com'], headless=True, user_data_dir=None)
	watchdog = SecurityWatchdog(browser_session=BrowserSession(browser_profile=profile), event_bus=EventBus())
	assert watchdog._is_url_allowed('https://example.com/path')
	assert watchdog._is_url_allowed('https://example.com:443/path')
	assert not watchdog._is_url_allowed('https://example.com:8443/path')
	assert not watchdog._is_url_allowed('https://example.com.evil.test/path')
	assert not watchdog._is_url_allowed('https://example.com@evil.test/path')


def test_large_domain_list_retains_wildcard_policy():
	patterns = ['*.example.com', *[f'other-{index}.test' for index in range(100)]]
	profile = BrowserProfile(prohibited_domains=patterns, headless=True, user_data_dir=None)
	watchdog = SecurityWatchdog(browser_session=BrowserSession(browser_profile=profile), event_bus=EventBus())
	assert isinstance(profile.prohibited_domains, list)
	assert not watchdog._is_url_allowed('https://sub.example.com/path')


def test_malformed_config_is_preserved(tmp_path):
	path = tmp_path / 'config.json'
	path.write_text('{not-json')
	load_and_migrate_config(path)
	assert path.read_text() == '{not-json'


def test_new_config_is_owner_only(tmp_path):
	path = tmp_path / 'config.json'
	load_and_migrate_config(path)
	assert path.stat().st_mode & 0o077 == 0


def test_agent_telemetry_omits_user_content():
	event = AgentTelemetryEvent(
		task='private task',
		model='model',
		model_provider='provider',
		max_steps=1,
		max_actions_per_step=1,
		use_vision=False,
		version='1',
		source='test',
		cdp_url='private',
		agent_type=None,
		action_errors=['private'],
		action_history=[[{'input': 'private'}]],
		urls_visited=['private'],
		steps=1,
		total_input_tokens=1,
		total_output_tokens=1,
		prompt_cached_tokens=0,
		total_tokens=2,
		total_duration_seconds=1,
		success=True,
		final_result_response='private',
		error_message='private',
	)
	assert 'private' not in str(event.properties)
	assert event.properties['steps'] == 1


def test_telemetry_requires_opt_in_and_omits_parent_commands(monkeypatch):
	monkeypatch.delenv('ANONYMIZED_TELEMETRY', raising=False)
	assert CONFIG.ANONYMIZED_TELEMETRY is False
	event = MCPServerTelemetryEvent(version='1', action='start', parent_process_cmdline='private', error_message='private')
	assert 'private' not in str(event.properties)


def test_skill_cookie_scope_respects_domain_boundary_and_path():
	parameter = SimpleNamespace(cookie_domain='example.com', cookie_path='/account')
	assert SkillService._cookie_matches_scope({'domain': '.example.com', 'path': '/account'}, parameter)
	assert not SkillService._cookie_matches_scope({'domain': 'example.com.evil.test', 'path': '/account'}, parameter)
	assert not SkillService._cookie_matches_scope({'domain': '.example.com', 'path': '/other'}, parameter)
	assert not SkillService._cookie_matches_scope({'domain': '.example.com', 'path': '/accounting'}, parameter)
	assert not SkillService._cookie_matches_scope({'domain': '.example.com', 'path': '/'}, SimpleNamespace())
