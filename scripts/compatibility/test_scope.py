from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.compatibility.scope import ScopeError, build_worklist, write_immutable_json


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


class ScopeTests(unittest.TestCase):
    def make_suite(self) -> tuple[Path, Path]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        suite = Path(temporary.name)
        corpus = suite / "generated" / "prepared"
        payload = b"dicom-placeholder"
        digest = hashlib.sha256(payload).hexdigest()
        base_entry = {
            "case_id": "classic/sc/example",
            "path": "classic/sc/example/instance.dcm",
            "sha256": digest,
            "uids": {"sop_instance_uid": "2.25.1"},
            "dicom": {"sop_class_uid": "1.2.3"},
            "expected_capabilities": ["open_file"],
            "expected_semantics": {},
            "expected_visual_checks": {},
            "image": None,
            "known_stressors": [],
            "pixel_data": None,
            "recipe": {"recipe_id": "example"},
            "references": [],
        }
        for root_name in ("core", "extended"):
            root = corpus / root_name
            file_path = root / base_entry["path"]
            file_path.parent.mkdir(parents=True, exist_ok=True)
            file_path.write_bytes(payload)
            write_json(
                root / "manifest.json",
                {"files": [base_entry], "skipped_cases": []},
            )
        write_json(
            suite / "cases" / "registry.json",
            {
                "cases": [
                    {"case_id": "classic/sc/example", "status": "implemented"},
                    {
                        "case_id": "classic/sc/unprepared",
                        "status": "implemented",
                        "profiles": ["legacy"],
                        "requirements": {},
                    },
                    {
                        "case_id": "classic/sc/planned",
                        "status": "planned",
                        "profiles": ["extended"],
                        "requirements": {},
                    },
                ]
            },
        )
        return suite, corpus

    def test_deduplicates_only_identity_proven_occurrences(self) -> None:
        suite, corpus = self.make_suite()
        result = build_worklist(suite, corpus, enforce_prepared_baseline=False)
        self.assertEqual(result["summary"]["canonical_files"], 1)
        self.assertEqual(result["summary"]["manifest_occurrences"], 2)
        self.assertEqual(len(result["canonical_files"][0]["occurrences"]), 2)
        self.assertEqual(result["summary"]["implemented_unprepared"], 1)
        self.assertEqual(result["summary"]["planned_unavailable"], 1)

    def test_rejects_changed_corpus_bytes(self) -> None:
        suite, corpus = self.make_suite()
        target = corpus / "core" / "classic/sc/example/instance.dcm"
        target.write_bytes(b"changed")
        with self.assertRaisesRegex(ScopeError, "file hash mismatch"):
            build_worklist(suite, corpus, enforce_prepared_baseline=False)

    def test_typed_expected_fields_participate_in_contract_identity(self) -> None:
        suite, corpus = self.make_suite()
        extended_manifest = corpus / "extended" / "manifest.json"
        manifest = json.loads(extended_manifest.read_text(encoding="utf-8"))
        manifest["files"][0]["expected_geometry"] = {"slice_position_mm": 4.0}
        write_json(extended_manifest, manifest)
        result = build_worklist(suite, corpus, enforce_prepared_baseline=False)
        self.assertEqual(result["summary"]["canonical_files"], 2)
        self.assertEqual(
            {len(row["occurrences"]) for row in result["canonical_files"]}, {1}
        )

    def test_frozen_worklist_cannot_be_replaced(self) -> None:
        suite, _ = self.make_suite()
        output = suite / "artifacts" / "worklist.json"
        write_immutable_json(output, {"value": 1})
        write_immutable_json(output, {"value": 1})
        with self.assertRaisesRegex(ScopeError, "refusing to replace"):
            write_immutable_json(output, {"value": 2})


if __name__ == "__main__":
    unittest.main()
