"""Reject a release whose source or built artifacts differ from its tag."""

import argparse
import email
import tarfile
import tomllib
import zipfile
from pathlib import Path


def check_version(tag: str, root: Path = Path('.'), *, artifacts: bool = False) -> None:
	"""Check source metadata and, when requested, both distribution formats."""
	expected = tag.removeprefix('v')
	versions = {'pyproject.toml': tomllib.loads((root / 'pyproject.toml').read_text())['project']['version']}
	if artifacts:
		wheels = list((root / 'dist').glob('*.whl'))
		sdists = list((root / 'dist').glob('*.tar.gz'))
		if not wheels or not sdists:
			raise ValueError('Release requires both wheel and source distribution artifacts')
		for path in wheels:
			with zipfile.ZipFile(path) as archive:
				metadata = next(name for name in archive.namelist() if name.endswith('.dist-info/METADATA'))
				versions[path.name] = email.message_from_bytes(archive.read(metadata))['Version']
		for path in sdists:
			with tarfile.open(path) as archive:
				metadata = next(member for member in archive.getmembers() if member.name.endswith('/PKG-INFO'))
				file = archive.extractfile(metadata)
				if file is None:
					raise ValueError(f'{path.name} is missing PKG-INFO')
				versions[path.name] = email.message_from_bytes(file.read())['Version']
	for name, version in versions.items():
		if version != expected:
			raise ValueError(f'{name} version {version} does not match release tag {expected}')


if __name__ == '__main__':
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument('tag')
	parser.add_argument('--artifacts', action='store_true')
	args = parser.parse_args()
	check_version(args.tag, artifacts=args.artifacts)
