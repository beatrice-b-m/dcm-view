from __future__ import annotations

import unittest
from argparse import Namespace
from unittest.mock import Mock, patch

from scripts.compatibility.stress_runner import cache_pressure_frames, cancellation_probe, deterministic_frames


class StressRunnerTests(unittest.TestCase):
    def test_frame_selection_is_bounded_deterministic_and_covers_boundaries(self) -> None:
        first = deterministic_frames("stress/example", 256)
        self.assertEqual(first, deterministic_frames("stress/example", 256))
        self.assertEqual(first[:3], [0, 128, 255])
        self.assertLessEqual(len(first), 4)

    def test_empty_and_single_frame_selections_do_not_duplicate(self) -> None:
        self.assertEqual(deterministic_frames("case", 0), [])
        self.assertEqual(deterministic_frames("case", 1), [0])

    def test_cache_pressure_spans_frame_range_with_a_hard_request_cap(self) -> None:
        self.assertEqual(cache_pressure_frames(1000, 4), [0, 333, 666, 999])
        self.assertEqual(cache_pressure_frames(2, 16), [0, 1])

    @patch("scripts.compatibility.stress_runner.bounded_get")
    def test_cancellation_requires_observed_incomplete_discovery(self, get: Mock) -> None:
        process = Mock()
        process.wait_for_url.return_value = "http://127.0.0.1:1234"
        process.shutdown.return_value = {"exit_code": -15, "forced": False, "elapsed_ms": 1}
        args = Namespace(startup_timeout=1, request_timeout=1, max_response_bytes=1024, shutdown_timeout=1)
        get.return_value = {"status": 200, "json": {"scan_complete": False}}
        observed = cancellation_probe(process, args)
        self.assertTrue(observed["requested_during_discovery"])

        get.return_value = {"status": 200, "json": {"scan_complete": True}}
        completed = cancellation_probe(process, args)
        self.assertFalse(completed["requested_during_discovery"])
        self.assertIsNotNone(completed["error"])


if __name__ == "__main__": unittest.main()
