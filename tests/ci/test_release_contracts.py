"""Release metadata and wiring must fail closed before publishing."""

import importlib.util
import io
import os
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('check_release_version', ROOT / 'scripts/check_release_version.py')
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class ReleaseContracts(unittest.TestCase):
	def test_source_and_artifacts_match_tag(self):
		with tempfile.TemporaryDirectory() as directory:
			root = Path(directory)
			(root / 'pyproject.toml').write_text('[project]\nversion = "1.2.3rc1"\n')
			(root / 'dist').mkdir()
			with zipfile.ZipFile(root / 'dist/package.whl', 'w') as wheel:
				wheel.writestr('package.dist-info/METADATA', 'Version: 1.2.3rc1\n')
			with tarfile.open(root / 'dist/package.tar.gz', 'w:gz') as sdist:
				data = b'Version: 1.2.3rc1\n'
				info = tarfile.TarInfo('package/PKG-INFO')
				info.size = len(data)
				sdist.addfile(info, io.BytesIO(data))
			CHECK.check_version('v1.2.3rc1', root, artifacts=True)
			with self.assertRaisesRegex(ValueError, 'does not match'):
				CHECK.check_version('1.2.3', root, artifacts=True)
			with zipfile.ZipFile(root / 'dist/package.whl', 'w') as wheel:
				wheel.writestr('package.dist-info/METADATA', 'Version: 1.2.2\n')
			with self.assertRaisesRegex(ValueError, 'package.whl'):
				CHECK.check_version('1.2.3rc1', root, artifacts=True)

	def test_tag_script_creates_one_version_or_validates_existing_tag(self):
		import yaml

		workflow = yaml.safe_load((ROOT / '.github/workflows/publish.yml').read_text())
		script = workflow['jobs']['tag_pre_release']['steps'][1]['run']
		with tempfile.TemporaryDirectory() as directory:
			root = Path(directory)
			remote = root / 'origin.git'
			repo = root / 'repo'
			repo.mkdir()

			def git(*args, cwd=repo):
				return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True)

			git('init', '--bare', str(remote), cwd=root)
			git('init')
			git('config', 'user.name', 'Release Fixture')
			git('config', 'user.email', 'fixture@example.invalid')
			git('remote', 'add', 'origin', str(remote))
			(repo / 'pyproject.toml').write_text('[project]\nversion = "1.2.3rc1"\n')
			(repo / 'scripts').mkdir()
			shutil.copyfile(ROOT / 'scripts/check_release_version.py', repo / 'scripts/check_release_version.py')
			git('add', '.')
			git('commit', '-m', 'fixture')
			env = {**os.environ, 'CREATE_TAG': 'true', 'GITHUB_OUTPUT': str(root / 'output')}

			def run():
				return subprocess.run(['bash', '-c', script], cwd=repo, env=env, capture_output=True, text=True)

			self.assertEqual(run().returncode, 0)
			self.assertEqual(git('rev-parse', '1.2.3rc1').stdout, git('rev-parse', 'HEAD').stdout)
			self.assertNotEqual(run().returncode, 0)
			env.update(CREATE_TAG='false', RELEASE_REF_TYPE='branch', RELEASE_REF_NAME='main')
			self.assertNotEqual(run().returncode, 0)
			env.update(RELEASE_REF_TYPE='tag', RELEASE_REF_NAME='1.2.3rc1')
			self.assertEqual(run().returncode, 0)
			env['RELEASE_REF_NAME'] = '1.2.2'
			self.assertNotEqual(run().returncode, 0)

	def test_workflow_and_docker_use_validated_inputs(self):
		workflow = (ROOT / '.github/workflows/publish.yml').read_text()
		self.assertIn('needs: tag_pre_release', workflow)
		self.assertIn('group: publish-${{ github.repository }}', workflow)
		self.assertIn('cancel-in-progress: false', workflow)
		self.assertIn('ref: ${{ needs.tag_pre_release.outputs.new_tag || github.ref }}', workflow)
		self.assertIn('check_release_version.py "$EXPECTED_TAG" --artifacts', workflow)
		self.assertIn('git rev-parse --verify "refs/tags/$new_tag"', workflow)
		self.assertNotIn('\nuv.lock\n', (ROOT / '.gitignore').read_text())
		fast = (ROOT / 'Dockerfile.fast').read_text()
		self.assertNotIn('BASE_TAG=latest', fast)
		self.assertIn('sha256sum --check /app/base-uv-lock.sha256', fast)
		self.assertIn('ARG BASE_IMAGE', fast)
		for name in ['Dockerfile', 'Dockerfile.fast', 'docker/base-images/python-deps/Dockerfile']:
			text = (ROOT / name).read_text()
			self.assertIn('COPY pyproject.toml uv.lock ', text)
			self.assertIn('uv sync --all-extras --locked', text)


if __name__ == '__main__':
	unittest.main()
