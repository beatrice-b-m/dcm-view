from __future__ import annotations
import json, unittest
from pathlib import Path
from scripts.compatibility.assertions import ASSERTIONS, evaluate, validate_registry
from scripts.compatibility.policy import load_policy

class AssertionRegistryTests(unittest.TestCase):
    def test_policy_and_fresh_manifest_capabilities_are_registered(self) -> None:
        root = Path("artifacts/compatibility/2026-08-28/corpus/all/manifest.json")
        rows = json.loads(root.read_text())["files"] if root.exists() else [{"expected_capabilities": ["open_file", "read_metadata"]}]
        validate_registry(load_policy(), rows)
    def test_evaluator_requires_concrete_evidence(self) -> None:
        self.assertEqual(evaluate("cache_miss_hit", {"display_cache": True, "raw_cache": True})["status"], "passed")
        self.assertEqual(evaluate("cache_miss_hit", {"display_cache": False})["status"], "failed")
    def test_registry_has_no_known_gap_assertion(self) -> None:
        self.assertNotIn("known_gap", ASSERTIONS)

if __name__ == "__main__": unittest.main()
