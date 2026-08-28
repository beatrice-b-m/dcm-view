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

    def test_required_assertions_may_be_not_applicable_for_a_case(self) -> None:
        worklist = {"suite": {"commit": "a"*40}, "inputs": {"policy_sha256": "b"*64, "manifests": [{"profile": "all", "sha256": "c"*64}]}, "files": [{"manifest_identity": {"profile": "all"}, "case_id": "classic/sc/x", "path": "x.dcm", "expected_contract": {"dicom": {}}, "policy": {"classification": "pixel_faithful_interactive", "required_assertions": ["reference_closure"], "semantic_context_assertions": [], "expected_unsupported": None, "rule_id": "image"}}]}
        base = {"generated_at": "now", "viewer": {"version": "dcmview 1", "sha256": "d"*64, "binary": "/bin/dcmview"}, "run": {"started_at": "a", "completed_at": "b", "command": ["dcmview"], "timeouts_seconds": {"shard": 1}}, "artifacts": [], "results": [{"root": "all", "case_id": "classic/sc/x", "path": "x.dcm", "execution_safety": "safe", "observations": {}, "http": {}, "timings_ms": {"total": 1}, "errors": []}]}
        evidence = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(evidence["results"][0]["assertions"][0]["status"], "not_applicable")
        self.assertEqual(evidence["results"][0]["status"], "passed")

    def test_generic_series_and_presentation_assertions_are_conditional(self) -> None:
        from scripts.compatibility.reports import assertion_evidence
        evidence = assertion_evidence({"series_navigation": {"mapped": True, "capabilities": {}}, "png_dimensions": {"passed": True}})
        self.assertIsNone(evidence["series_navigation"])
        self.assertIsNone(evidence["presentation_checks"])
        self.assertEqual(evidence["normalized_display_hash"], {"passed": True})

if __name__ == "__main__": unittest.main()
