from __future__ import annotations

import argparse
import unittest

from scripts.compatibility.fuzz_runner import candidates, qualification_budget
from scripts.compatibility.robustness import RobustnessError


class FuzzRunnerTests(unittest.TestCase):
    def test_candidate_generation_is_reproducible_and_bounded(self) -> None:
        first = list(candidates(b"DICM payload", 7, 8, 4, 64))
        self.assertEqual(first, list(candidates(b"DICM payload", 7, 8, 4, 64)))
        self.assertEqual(len(first), 8)
        self.assertTrue(all(len(payload) <= 64 and 1 <= mutations <= 4 for payload, mutations in first))

    def test_adapter_bounds_cannot_exceed_upstream_qualification(self) -> None:
        qualification = {"contract": {"budget": {"max_candidates": 64, "max_mutations_per_candidate": 8, "max_input_bytes": 1024, "max_total_target_operations": 4096}}}
        args = argparse.Namespace(max_candidates=8, max_mutations_per_candidate=4, max_input_bytes=512, max_total_target_operations=2048)
        self.assertEqual(qualification_budget(qualification, args)["max_candidates"], 8)
        args.max_candidates = 65
        with self.assertRaisesRegex(RobustnessError, "exceeds qualification ceiling"):
            qualification_budget(qualification, args)

    def test_oversized_seed_is_rejected_before_mutation(self) -> None:
        with self.assertRaisesRegex(RobustnessError, "seed input exceeds"):
            list(candidates(b"12345", 1, 1, 1, 4))


if __name__ == "__main__": unittest.main()
