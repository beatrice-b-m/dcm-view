from __future__ import annotations

import unittest
from unittest import mock

from scripts import check


class RecordingRunner(check.CheckRunner):
	def __init__(self) -> None:
		super().__init__(install=False)
		self.calls: list[str] = []

	@property
	def cargo(self) -> str:
		return "cargo"

	def versions(self) -> None:
		self.calls.append("versions")

	def frontend(self) -> None:
		self.calls.append("frontend")

	def build_frontend(self) -> None:
		self.calls.append("frontend-assets")

	def rust_lint(self) -> None:
		self.calls.append("rust-lint")

	def rust_test(self) -> None:
		self.calls.append("rust-test")

	def python_unit(self) -> None:
		self.calls.append("python-unit")

	def python_integration(self) -> None:
		self.calls.append("python-integration")

	def vscode_compile(self) -> None:
		self.calls.append("vscode")

	def vscode_integration(self) -> None:
		self.calls.append("vscode-integration")


class CheckProfileCompositionTests(unittest.TestCase):
	def test_aggregate_profiles_compose_the_documented_layers(self) -> None:
		cases = {
			"quick": [
				"versions",
				"frontend",
				"rust-lint",
				"python-unit",
			],
			"core": [
				"versions",
				"frontend",
				"rust-lint",
				"rust-test",
				"python-unit",
				"vscode",
			],
			"e2e": [
				"versions",
				"frontend",
				"rust-lint",
				"rust-test",
				"python-unit",
				"vscode",
				"python-integration",
				"vscode-integration",
			],
		}

		for profile, expected in cases.items():
			with self.subTest(profile=profile):
				runner = RecordingRunner()
				getattr(runner, profile)()
				self.assertEqual(runner.calls, expected)

	def test_external_is_an_independent_remote_fixture_profile(self) -> None:
		runner = RecordingRunner()

		with mock.patch.object(check, "run") as run:
			runner.external()

		self.assertEqual(runner.calls, ["frontend-assets"])
		run.assert_called_once()
		label, command = run.call_args.args
		self.assertEqual(label, "Run feature-gated remote fixture tests")
		self.assertEqual(
			command,
			[
				"cargo",
				"test",
				"--locked",
				"--features",
				"remote-fixtures",
				"--test",
				"integration",
				"--",
				"--ignored",
			],
		)


if __name__ == "__main__":
	unittest.main()
