from __future__ import annotations

import unittest

from scripts.compatibility.policy import PolicyError, load_policy, resolve


class PolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.policy = load_policy()

    def test_resolves_semantic_preview_before_generic_image(self) -> None:
        row = {"case_id": "derived/seg/example", "dicom": {"sop_class_uid": "1.2.840.10008.5.1.4.1.1.66.4", "transfer_syntax_uid": "1.2.840.10008.1.2.1"}, "image": {"bits_allocated": 1}}
        self.assertEqual(resolve(self.policy, row, "all")["classification"], "pixel_preview")

    def test_resolves_unqualified_transfer_syntax_as_controlled(self) -> None:
        row = {"case_id": "classic/sc/lossy", "dicom": {"sop_class_uid": "1.2.840.10008.5.1.4.1.1.7", "transfer_syntax_uid": "1.2.840.10008.1.2.4.112"}, "image": {"bits_allocated": 8}}
        rule = resolve(self.policy, row, "all")
        self.assertEqual(rule["classification"], "controlled_unsupported")
        self.assertEqual(rule["expected_unsupported"]["statuses"], [422])

    def test_rejects_unmatched_profile(self) -> None:
        with self.assertRaisesRegex(PolicyError, "no policy outcome"):
            resolve(self.policy, {"case_id": "x/y"}, "unknown")


if __name__ == "__main__":
    unittest.main()
