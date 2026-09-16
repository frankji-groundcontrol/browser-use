"""Release metadata and wiring must fail closed before publishing."""

import importlib.util
import io
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('check_release_version', ROOT / 'scripts/check_release_version.py')
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

	def test_workflow_and_docker_use_validated_inputs(self):
		workflow = (ROOT / '.github/workflows/publish.yml').read_text()
		self.assertIn('needs: tag_pre_release', workflow)
		self.assertIn('ref: ${{ needs.tag_pre_release.outputs.new_tag || github.ref }}', workflow)
		self.assertIn('check_release_version.py "$EXPECTED_TAG" --artifacts', workflow)
		self.assertIn('git rev-parse --verify "refs/tags/$new_tag"', workflow)
		self.assertNotIn('\nuv.lock\n', (ROOT / '.gitignore').read_text())
		for name in ['Dockerfile', 'Dockerfile.fast', 'docker/base-images/python-deps/Dockerfile']:
			text = (ROOT / name).read_text()
			self.assertIn('COPY pyproject.toml uv.lock ', text)
			self.assertIn('uv sync --all-extras --locked', text)


if __name__ == '__main__':
	unittest.main()
