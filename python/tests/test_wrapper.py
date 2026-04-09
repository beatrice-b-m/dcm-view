from __future__ import annotations

import os
import shutil
import subprocess
import sys
import time
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
PYTHON_SRC = REPO_ROOT / "python"
if str(PYTHON_SRC) not in sys.path:
	sys.path.insert(0, str(PYTHON_SRC))

from dcmview_py import __main__ as dcmview_main
from dcmview_py import wrapper

FIXTURE_FILE = REPO_ROOT / "FFDM_R_MLO_ComboHD.dcm"


def _available_dcmview_binary() -> Path | None:
	for candidate in [
		REPO_ROOT / "target" / "debug" / "dcmview",
		REPO_ROOT / "target" / "release" / "dcmview",
	]:
		if candidate.is_file():
			return candidate

	resolved = shutil.which("dcmview")
	if resolved is None:
		return None
	return Path(resolved)


class WrapperTests(unittest.TestCase):
	def test_missing_binary_raises_runtime_error(self) -> None:
		with mock.patch("dcmview_py.wrapper.shutil.which", return_value=None):
			with self.assertRaisesRegex(RuntimeError, "dcmview binary not found"):
				wrapper.view([FIXTURE_FILE], browser=False)

	def test_tunnel_requires_host_before_spawn(self) -> None:
		with mock.patch("dcmview_py.wrapper.shutil.which", return_value="/tmp/dcmview"):
			with self.assertRaisesRegex(ValueError, "tunnel_host is required"):
				wrapper.view([FIXTURE_FILE], browser=False, tunnel=True)

	def test_non_blocking_launch_captures_url_and_stop(self) -> None:
		binary = _available_dcmview_binary()
		if binary is None:
			self.skipTest("dcmview binary not available")
		if not FIXTURE_FILE.is_file():
			self.skipTest("fixture DICOM file not found")

		with mock.patch.dict(
			os.environ,
			{"PATH": f"{binary.parent}{os.pathsep}{os.environ.get('PATH', '')}"},
			clear=False,
		):
			handle = wrapper.view([FIXTURE_FILE], browser=False, timeout=30, block=False)

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

	def test_cli_forwards_no_browser_no_recursive_and_timeout(self) -> None:
		with mock.patch("dcmview_py.__main__.view", return_value=None) as view_mock:
			exit_code = dcmview_main.run_cli(
				[
					"--no-browser",
					"--no-recursive",
					"--timeout",
					"9",
					"-p",
					"1042",
					"--host",
					"0.0.0.0",
					str(FIXTURE_FILE),
				]
			)

		self.assertEqual(exit_code, 0)
		view_mock.assert_called_once_with(
			[str(FIXTURE_FILE)],
			port=1042,
			host="0.0.0.0",
			browser=False,
			tunnel=False,
			tunnel_host=None,
			tunnel_port=0,
			recursive=False,
			timeout=9,
			block=True,
		)

	def test_cli_returns_child_exit_code(self) -> None:
		with mock.patch(
			"dcmview_py.__main__.view",
			side_effect=subprocess.CalledProcessError(7, ["dcmview"]),
		):
			exit_code = dcmview_main.run_cli([str(FIXTURE_FILE)])

		self.assertEqual(exit_code, 7)


if __name__ == "__main__":
	unittest.main()
