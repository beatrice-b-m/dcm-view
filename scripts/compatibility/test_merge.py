from __future__ import annotations

import hashlib
import unittest

from scripts.compatibility.merge import merge_worklists
from scripts.compatibility.scope import ScopeError, canonical_json, contract_sha256


def row(case_id: str, path: str, sop_uid: str, payload: str) -> dict:
    contract = {
        "case_id": case_id,
        "dicom": {"transfer_syntax_uid": "1.2.840.10008.1.2.5"},
        "image": {"rows": 2, "columns": 2},
        "uids": {"sop_instance_uid": sop_uid},
        "pixel_data": {"payload": payload},
    }
    return {
        "case_id": case_id,
        "sha256": hashlib.sha256(payload.encode()).hexdigest(),
        "contract_sha256": contract_sha256(contract),
        "sop_instance_uid": sop_uid,
        "expected_contract": contract,
        "selected": {"path": path, "normalized_path": f"/corpus/{payload}/{path}"},
        "occurrences": [],
    }


def worklist(rows: list[dict], label: str) -> dict:
    value = {
        "worklist_schema_version": "0.1.0",
        "corpus": {"label": label},
        "summary": {"canonical_files": len(rows)},
        "canonical_files": rows,
        "unavailable": [],
    }
    value["worklist_sha256"] = hashlib.sha256(canonical_json(value)).hexdigest()
    return value


class MergeWorklistTests(unittest.TestCase):
    def test_replaces_changed_overlap_and_preserves_canonical_inventory(self) -> None:
        unchanged = row("case/a", "a.dcm", "1.2.3", "same")
        original = row("case/b", "b.dcm", "1.2.4", "old")
        corrected = row("case/b", "b.dcm", "1.2.4", "new")
        base = worklist([unchanged, original], "base")
        overlay = worklist([unchanged, corrected], "overlay")

        merged = merge_worklists(base, overlay)

        self.assertEqual(len(merged["canonical_files"]), 2)
        self.assertEqual(merged["canonical_files"][0], unchanged)
        self.assertEqual(merged["canonical_files"][1], corrected)
        self.assertEqual(merged["merge"]["overlap_files"], 2)
        self.assertEqual(merged["merge"]["replacement_files"], 1)
        declared_hash = merged["worklist_sha256"]
        unhashed = {key: value for key, value in merged.items() if key != "worklist_sha256"}
        self.assertEqual(declared_hash, hashlib.sha256(canonical_json(unhashed)).hexdigest())

    def test_rejects_overlay_outside_base_identity(self) -> None:
        base = worklist([row("case/a", "a.dcm", "1.2.3", "old")], "base")
        overlay = worklist([row("case/b", "b.dcm", "1.2.4", "new")], "overlay")
        with self.assertRaisesRegex(ScopeError, "absent from the canonical base"):
            merge_worklists(base, overlay)

    def test_rejects_changes_to_stable_contract_identity(self) -> None:
        original = row("case/a", "a.dcm", "1.2.3", "old")
        corrected = row("case/a", "a.dcm", "1.2.3", "new")
        corrected["expected_contract"]["image"]["rows"] = 3
        corrected["contract_sha256"] = contract_sha256(corrected["expected_contract"])
        with self.assertRaisesRegex(ScopeError, "stable contract identity"):
            merge_worklists(
                worklist([original], "base"), worklist([corrected], "overlay")
            )


if __name__ == "__main__":
    unittest.main()
