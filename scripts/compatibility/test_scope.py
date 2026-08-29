from __future__ import annotations
import hashlib, json, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
from scripts.compatibility.scope import ScopeError, applicable_assertions, build_worklist, load_worklist, write_immutable_json

def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True); path.write_text(json.dumps(value), encoding="utf-8")

class ScopeTests(unittest.TestCase):
    def test_selects_only_capability_backed_conditional_assertions(self) -> None:
        rule = {
            "required_assertions": ["discovery_identity"],
            "conditional_assertions": ["frame_navigation", "reference_closure"],
        }
        entry = {"expected_capabilities": ["navigate_multiframe"]}
        self.assertEqual(
            applicable_assertions(rule, entry),
            ["discovery_identity", "frame_navigation"],
        )

    def fixture(self, profile: str = "all", *, sop_uid: bool = True) -> tuple[Path, Path, Path]:
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup); base = Path(temporary.name); suite = base / "suite"; root = base / profile
        payload = b"dicom"; path = root / "case/a.dcm"; path.parent.mkdir(parents=True); path.write_bytes(payload)
        entry = {"case_id": "classic/sc/example", "path": "case/a.dcm", "sha256": hashlib.sha256(payload).hexdigest(), "dicom": {"sop_class_uid": "1.2.3", "transfer_syntax_uid": "1.2.840.10008.1.2.1"}, "image": {"bits_allocated": 8}}
        if sop_uid: entry["uids"] = {"sop_instance_uid": "2.25.1"}
        manifest = {"manifest_schema_version": "0.2.0", "run": {"profile": profile}, "files": [entry], "qualifications": [], "skipped_cases": []}
        manifest_path = root / "manifest.json"; write_json(manifest_path, manifest); lock = base / "lock.json"
        write_json(lock, {"suite": {"commit": "a"*40}, "profiles": {profile: {"physical_files": 1, "logical_cases": 1, "qualifications": 0, "manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest()}}})
        return suite, root, lock
    def build(self, profile: str = "all", *, sop_uid: bool = True):
        suite, root, lock = self.fixture(profile, sop_uid=sop_uid)
        rule = {"id": "test", "precedence": 1, "match": {"profiles": [profile]}, "classification": "pixel_faithful_interactive", "required_assertions": [], "semantic_context_assertions": [], "expected_unsupported": None, "rationale": "test", "exclusions": []}
        with patch("scripts.compatibility.scope.verify_suite"), patch("scripts.compatibility.scope.load_policy", return_value={"rules": [rule]}), patch("scripts.compatibility.scope.policy_sha256", return_value="b"*64):
            return build_worklist(suite, {profile: root}, lock_path=lock)
    def test_preserves_profile_and_manifest_relative_identity(self) -> None:
        row = self.build()["files"][0]; self.assertEqual(row["manifest_identity"]["profile"], "all"); self.assertEqual(row["path"], "case/a.dcm"); self.assertTrue(Path(row["normalized_path"]).is_absolute())
    def test_negative_input_does_not_require_sop_instance_uid(self) -> None:
        self.assertIsNone(self.build("negative", sop_uid=False)["models"]["negative_inputs"][0]["sop_instance_uid"])
    def test_profile_isolation_does_not_invent_unavailable_rows(self) -> None:
        result = self.build("legacy"); self.assertEqual(result["inputs"]["profiles"], ["legacy"]); self.assertEqual(result["unavailable"], [])
    def test_rejects_historical_worklist_schema(self) -> None:
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup); path = Path(temporary.name) / "old.json"; write_json(path, {"worklist_schema_version": "0.1.0"})
        with self.assertRaisesRegex(ScopeError, "incompatible worklist schema"): load_worklist(path)
    def test_frozen_worklist_cannot_be_replaced(self) -> None:
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup); output = Path(temporary.name) / "worklist.json"; write_immutable_json(output, {"value": 1}); write_immutable_json(output, {"value": 1})
        with self.assertRaisesRegex(ScopeError, "refusing to replace"): write_immutable_json(output, {"value": 2})

if __name__ == "__main__": unittest.main()
