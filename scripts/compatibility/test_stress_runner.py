from __future__ import annotations

import unittest

from scripts.compatibility.stress_runner import deterministic_frames


class StressRunnerTests(unittest.TestCase):
    def test_frame_selection_is_bounded_deterministic_and_covers_boundaries(self) -> None:
        first = deterministic_frames("stress/example", 256)
        self.assertEqual(first, deterministic_frames("stress/example", 256))
        self.assertEqual(first[:3], [0, 128, 255])
        self.assertLessEqual(len(first), 4)

    def test_empty_and_single_frame_selections_do_not_duplicate(self) -> None:
        self.assertEqual(deterministic_frames("case", 0), [])
        self.assertEqual(deterministic_frames("case", 1), [0])


if __name__ == "__main__": unittest.main()
