from __future__ import annotations

import hashlib
import importlib.util
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PACKAGE_RELEASE_ARCHIVE = REPO_ROOT / "scripts" / "package_release_archive.py"


def _load_archive_module():
	spec = importlib.util.spec_from_file_location(
		"package_release_archive_for_test",
		PACKAGE_RELEASE_ARCHIVE,
	)
	assert spec is not None
	module = importlib.util.module_from_spec(spec)
	assert spec.loader is not None
	sys.modules[spec.name] = module
	spec.loader.exec_module(module)
	return module


class ReleaseArchiveTests(unittest.TestCase):
	def test_writes_windows_zip_and_sha256(self) -> None:
		packager = _load_archive_module()
		with tempfile.TemporaryDirectory(prefix="dcmview-release-archive-") as temp_dir:
			root = Path(temp_dir)
			binary = root / "dcmview.exe"
			binary.write_bytes(b"windows-binary")
			output = root / "dcmview-0.2.8-x86_64-pc-windows-msvc.zip"

			packager.write_zip(output, packager.archive_members(binary, "dcmview.exe"))
			sha_path = output.with_name(f"{output.name}.sha256")
			sha_path.write_text(f"{packager.sha256_file(output)}\n", encoding="utf-8")

			with zipfile.ZipFile(output) as archive:
				self.assertEqual(archive.read("dcmview.exe"), b"windows-binary")
				self.assertIn("README.md", archive.namelist())
				self.assertIn("LICENSE", archive.namelist())

			expected_sha = hashlib.sha256(output.read_bytes()).hexdigest()
			self.assertEqual(sha_path.read_text(encoding="utf-8").strip(), expected_sha)


if __name__ == "__main__":
	unittest.main()
