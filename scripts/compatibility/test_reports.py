from __future__ import annotations
import unittest
from scripts.compatibility.reports import build_evidence_report, build_viewer_report

class ReportTests(unittest.TestCase):
    def test_expected_unsupported_is_success_in_suite_report(self) -> None:
        worklist = {"suite": {"commit": "a"*40}, "inputs": {"policy_sha256": "b"*64, "manifests": [{"profile": "all", "sha256": "c"*64}]}, "files": [{"manifest_identity": {"profile": "all"}, "case_id": "classic/sc/x", "path": "x.dcm", "expected_contract": {"dicom": {"transfer_syntax_uid": "unsupported"}}, "policy": {"classification": "controlled_unsupported", "required_assertions": [], "semantic_context_assertions": [], "expected_unsupported": {"statuses": [422]}, "rule_id": "unsupported"}}]}
        base = {"generated_at": "now", "viewer": {"version": "dcmview 1", "sha256": "d"*64, "binary": "/bin/dcmview"}, "run": {"started_at": "a", "completed_at": "b", "command": ["dcmview"], "timeouts_seconds": {"shard": 1}}, "artifacts": [], "results": [{"root": "all", "case_id": "classic/sc/x", "path": "x.dcm", "execution_safety": "safe", "observations": {}, "http": {"display": {"status": 422}}, "timings_ms": {"total": 1}, "errors": []}]}
        evidence = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(evidence["results"][0]["status"], "expected_unsupported")
        self.assertEqual(build_viewer_report(evidence)["summary"]["passed"], 1)

if __name__ == "__main__": unittest.main()
