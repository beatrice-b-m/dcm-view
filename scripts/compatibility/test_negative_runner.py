from __future__ import annotations

import io
import unittest

from scripts.compatibility.negative_runner import acceptable_outcomes, classify
from scripts.compatibility.robustness import BoundedProcess


class NegativeRunnerTests(unittest.TestCase):
    def test_accepts_union_of_declared_mutation_outcomes(self) -> None:
        entry = {"expected_contract": {"negative_evidence": {"mutation_steps": [
            {"acceptable_outcomes": ["parse_failure"]}, {"acceptable_outcomes": ["decode_failure", "parse_failure"]},
        ]}}}
        self.assertEqual(acceptable_outcomes(entry), {"parse_failure", "decode_failure"})

    def test_classifies_discovery_skip_and_bounded_decode_error(self) -> None:
        self.assertEqual(classify(False, None), "clean_rejection")
        self.assertEqual(classify(True, {"status": 500, "json": {"error": "pixel decode failed"}}), "decode_failure")
        token_error = {"status": 500, "json": {"error": "Could not read data set token"}}
        self.assertEqual(classify(True, token_error, "dataset_parser"), "parse_failure")
        self.assertEqual(
            classify(True, token_error, "encapsulated_value_parser"),
            "decode_failure",
        )

    def test_drain_discards_bytes_past_per_stream_limit(self) -> None:
        process = object.__new__(BoundedProcess)
        process._limit = 4
        process._condition = __import__("threading").Condition()
        process._discarded = {"stdout": 0, "stderr": 0}
        target = bytearray()
        process._drain(io.BytesIO(b"abcdefgh"), target, "stdout")
        self.assertEqual(target, b"abcd")
        self.assertEqual(process._discarded["stdout"], 4)


if __name__ == "__main__": unittest.main()
