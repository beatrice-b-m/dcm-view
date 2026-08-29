from __future__ import annotations
import unittest
from scripts.compatibility.reports import build_evidence_report, build_viewer_report

class ReportTests(unittest.TestCase):
    def test_expected_unsupported_is_success_in_suite_report(self) -> None:
        worklist = {"suite": {"commit": "a"*40}, "inputs": {"policy_sha256": "b"*64, "manifests": [{"profile": "all", "sha256": "c"*64}]}, "files": [{"manifest_identity": {"profile": "all"}, "case_id": "classic/sc/x", "path": "x.dcm", "expected_contract": {"dicom": {"transfer_syntax_uid": "unsupported"}}, "policy": {"classification": "controlled_unsupported", "required_assertions": ["controlled_unsupported_error"], "semantic_context_assertions": [], "expected_unsupported": {"statuses": [422]}, "rule_id": "unsupported"}}]}
        base = {"generated_at": "now", "viewer": {"version": "dcmview 1", "sha256": "d"*64, "binary": "/bin/dcmview"}, "run": {"started_at": "a", "completed_at": "b", "command": ["dcmview"], "timeouts_seconds": {"shard": 1}}, "artifacts": [], "results": [{"root": "all", "case_id": "classic/sc/x", "path": "x.dcm", "execution_safety": "safe", "observations": {"controlled_unsupported": {"passed": True}}, "http": {"display": {"status": 422}}, "timings_ms": {"total": 1}, "errors": []}]}
        evidence = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(evidence["results"][0]["status"], "expected_unsupported")
        self.assertEqual(build_viewer_report(evidence)["summary"]["passed"], 1)

        base["results"][0]["observations"] = {}
        missing = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(missing["results"][0]["status"], "failed")

    def test_required_assertions_fail_when_evidence_is_absent(self) -> None:
        worklist = {"suite": {"commit": "a"*40}, "inputs": {"policy_sha256": "b"*64, "manifests": [{"profile": "all", "sha256": "c"*64}]}, "files": [{"manifest_identity": {"profile": "all"}, "case_id": "classic/sc/x", "path": "x.dcm", "expected_contract": {"dicom": {}}, "policy": {"classification": "pixel_faithful_interactive", "required_assertions": ["reference_closure"], "semantic_context_assertions": [], "expected_unsupported": None, "rule_id": "image"}}]}
        base = {"generated_at": "now", "viewer": {"version": "dcmview 1", "sha256": "d"*64, "binary": "/bin/dcmview"}, "run": {"started_at": "a", "completed_at": "b", "command": ["dcmview"], "timeouts_seconds": {"shard": 1}}, "artifacts": [], "results": [{"root": "all", "case_id": "classic/sc/x", "path": "x.dcm", "execution_safety": "safe", "observations": {}, "http": {}, "timings_ms": {"total": 1}, "errors": []}]}
        evidence = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(evidence["results"][0]["assertions"][0]["status"], "failed")
        self.assertEqual(evidence["results"][0]["status"], "failed")

    def test_generic_series_and_presentation_assertions_are_conditional(self) -> None:
        from scripts.compatibility.reports import assertion_evidence
        evidence = assertion_evidence({"series_navigation": {"mapped": True, "capabilities": {}}, "png_dimensions": {"passed": True}})
        self.assertIsNone(evidence["series_navigation"])
        self.assertIsNone(evidence["presentation_checks"])
        self.assertEqual(evidence["normalized_display_hash"], {"passed": True})

    def test_error_envelope_alone_is_not_recovery_evidence(self) -> None:
        from scripts.compatibility.reports import assertion_evidence
        evidence = assertion_evidence({"error_envelope": True})
        self.assertIsNone(evidence["recovery_after_error"])

    def test_declared_semantic_assertions_require_concrete_evidence(self) -> None:
        worklist = {"suite": {"commit": "a"*40}, "inputs": {"policy_sha256": "b"*64, "manifests": [{"profile": "all", "sha256": "c"*64}]}, "files": [{"manifest_identity": {"profile": "all"}, "case_id": "derived/seg/x", "path": "x.dcm", "expected_contract": {"dicom": {}}, "policy": {"classification": "pixel_preview", "required_assertions": [], "semantic_context_assertions": ["segmentation_context"], "expected_unsupported": None, "rule_id": "seg"}}]}
        base = {"generated_at": "now", "viewer": {"version": "dcmview 1", "sha256": "d"*64, "binary": "/bin/dcmview"}, "run": {"started_at": "a", "completed_at": "b", "command": ["dcmview"], "timeouts_seconds": {"shard": 1}}, "artifacts": [], "results": [{"root": "all", "case_id": "derived/seg/x", "path": "x.dcm", "execution_safety": "safe", "observations": {}, "http": {}, "timings_ms": {"total": 1}, "errors": []}]}
        missing = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(missing["results"][0]["status"], "failed")
        self.assertEqual(missing["results"][0]["assertions"][0]["scope"], "semantic_context")

        base["results"][0]["observations"]["segmentation_context"] = {"passed": True}
        present = build_evidence_report(base, worklist, "e"*40, [])
        self.assertEqual(present["results"][0]["status"], "passed")

if __name__ == "__main__": unittest.main()
