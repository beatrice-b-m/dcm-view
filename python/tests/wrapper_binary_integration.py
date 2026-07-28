from __future__ import annotations

import os
import sys
import time
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
PYTHON_SRC = REPO_ROOT / "python"
if str(PYTHON_SRC) not in sys.path:
	sys.path.insert(0, str(PYTHON_SRC))

from dcmview_py import wrapper

FIXTURE_FILE = REPO_ROOT / "tests" / "fixtures" / "golden-uncompressed-u16-multiframe.dcm"


class WrapperBinaryIntegrationTests(unittest.TestCase):
	@classmethod
	def setUpClass(cls) -> None:
		if not FIXTURE_FILE.is_file():
			raise AssertionError(f"committed DICOM fixture is missing: {FIXTURE_FILE}")

		cls.binary = REPO_ROOT / "target" / "debug" / wrapper._binary_name()
		if not cls.binary.is_file():
			raise AssertionError(
				f"dcmview binary is missing: {cls.binary}; "
				"run `python scripts/check.py python-integration`"
			)

	def test_non_blocking_launch_captures_url_and_stops_cleanly(self) -> None:
		with mock.patch.dict(
			os.environ,
			{"DCMVIEW_BINARY": str(self.binary)},
			clear=False,
		):
			handle = wrapper.view(
				[FIXTURE_FILE],
				browser=False,
				timeout=30,
				block=False,
				vscode_bridge=False,
			)

			assert isinstance(handle, wrapper.ShutdownHandle)
			try:
				deadline = time.time() + 10.0
				while handle.url is None and time.time() < deadline:
					time.sleep(0.1)

				self.assertIsNotNone(handle.url)
				assert handle.url is not None
				self.assertTrue(handle.url.startswith("http://"))
			finally:
				exit_code = handle.stop()

			self.assertIsInstance(exit_code, int)
			self.assertIsInstance(handle.stop(), int)


if __name__ == "__main__":
	unittest.main()
