from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.compatibility.corpus import CorpusError, verify_manifest


class CorpusTests(unittest.TestCase):
    def fixture(self) -> tuple[Path, dict]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name) / "all"
        payload = b"dicom"
        path = root / "case/instance.dcm"
        path.parent.mkdir(parents=True)
        path.write_bytes(payload)
        manifest = {
            "manifest_schema_version": "0.2.0", "run": {"profile": "all"},
            "files": [{"case_id": "classic/sc/example", "path": "case/instance.dcm", "sha256": hashlib.sha256(payload).hexdigest()}],
            "qualifications": [],
        }
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        lock = {"profiles": {"all": {"physical_files": 1, "logical_cases": 1, "qualifications": 0, "manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest()}}}
        return root, lock

    def test_verifies_locked_manifest_and_payload(self) -> None:
        root, lock = self.fixture()
        self.assertEqual(verify_manifest("all", root, lock)["physical_files"], 1)

    def test_rejects_payload_hash_drift(self) -> None:
        root, lock = self.fixture()
        (root / "case/instance.dcm").write_bytes(b"changed")
        with self.assertRaisesRegex(CorpusError, "file hash mismatch"):
            verify_manifest("all", root, lock)

    def test_rejects_manifest_digest_drift(self) -> None:
        root, lock = self.fixture()
        manifest = json.loads((root / "manifest.json").read_text())
        manifest["extra"] = True
        (root / "manifest.json").write_text(json.dumps(manifest))
        with self.assertRaisesRegex(CorpusError, "manifest digest mismatch"):
            verify_manifest("all", root, lock)


if __name__ == "__main__":
    unittest.main()
