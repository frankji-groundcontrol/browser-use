"""REL-01: setup must create and verify one repository-root environment."""

import os
import shutil
import stat
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_setup_script_creates_and_verifies_one_repo_root_environment(tmp_path: Path):
	repo = tmp_path / 'repo'
	bin_dir = repo / 'bin'
	bin_dir.mkdir(parents=True)
	(repo / '.python-version').write_text('3.11\n')
	(bin_dir / 'lint.sh').write_text('#!/bin/sh\n')
	shutil.copy(ROOT / 'bin' / 'setup.sh', bin_dir / 'setup.sh')

	log = tmp_path / 'uv.log'
	fake_bin = tmp_path / 'fake-bin'
	fake_bin.mkdir()
	uv = fake_bin / 'uv'
	uv.write_text(
		f"""#!/usr/bin/env python3
import os, pathlib, sys
log = pathlib.Path({str(log)!r})
prev = log.read_text() if log.exists() else ''
log.write_text(prev + f"CWD={{os.getcwd()}} ENV={{os.environ.get('UV_PROJECT_ENVIRONMENT', '')}} ARGS={{sys.argv[1:]}}\\n")
args = sys.argv[1:]
if args[:1] == ['venv']:
	target = pathlib.Path(args[-1])
	bindir = target / 'bin'
	bindir.mkdir(parents=True)
	python = bindir / 'python'
	python.write_text('''#!/usr/bin/env python3
import os, sys, types
sys.prefix = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.exec_prefix = sys.prefix
if len(sys.argv) > 1 and sys.argv[1] == '-c':
	sys.modules['browser_use'] = types.ModuleType('browser_use')
	exec(sys.argv[2], {{'__name__': '__main__'}})
''')
	python.chmod(0o755)
	(bindir / 'activate').write_text('export VIRTUAL_ENV="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"\\nexport PATH="$VIRTUAL_ENV/bin:$PATH"\\n')
elif args[:1] == ['sync']:
	pass
elif args[:1] == ['pip']:
	print('Name: browser-use')
elif args[:1] == ['run']:
	i = 1
	while i < len(args) and args[i].startswith('--'):
		i += 1
	python = os.environ['UV_PROJECT_ENVIRONMENT'] + '/bin/python'
	os.execv(python, [python, *args[i + 1:]])
else:
	sys.exit(f'unexpected uv invocation {{args}}')
"""
	)
	uv.chmod(uv.stat().st_mode | stat.S_IEXEC)

	env = {**os.environ, 'PATH': f'{fake_bin}{os.pathsep}{os.environ.get("PATH", "")}'}
	completed = subprocess.run(['bash', str(bin_dir / 'setup.sh')], cwd=bin_dir, env=env, capture_output=True, text=True)
	assert completed.returncode == 0, completed.stdout + completed.stderr
	assert 'Verified browser_use in the project environment' in completed.stdout
	recorded = log.read_text()
	assert f'CWD={repo}' in recorded
	assert f'ENV={repo / ".venv"}' in recorded
	assert (repo / '.venv' / 'bin' / 'python').is_file()
	assert not (bin_dir / '.venv').exists()
	syntax = subprocess.run(['bash', '-n', str(ROOT / 'bin' / 'setup.sh')], capture_output=True, text=True)
	assert syntax.returncode == 0, syntax.stderr
