from __future__ import annotations

import struct
import subprocess
import sys
import unittest
import zlib
from pathlib import Path

from scripts.compatibility.run import (
    PROBED_CAPABILITIES,
    normalized_report,
    png_pixels,
    series_observation,
    validate_json_schema,
    validate_report,
    validate_visual,
)


def grayscale_png(values: bytes, width: int, height: int) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    rows = b"".join(b"\x00" + values[row * width : (row + 1) * width] for row in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


class RunnerTests(unittest.TestCase):
    def test_rle_capability_is_backed_by_display_and_lossless_raw_probes(self) -> None:
        self.assertIn("decode_rle_lossless_pixels", PROBED_CAPABILITIES)

    def test_runner_supports_documented_direct_invocation(self) -> None:
        runner = Path(__file__).with_name("run.py")
        completed = subprocess.run(
            [sys.executable, str(runner), "--help"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_png_decoder_and_smoke_gradient_check(self) -> None:
        width, height, pixels = png_pixels(grayscale_png(bytes((0, 85, 170, 255)), 2, 2))
        self.assertEqual((width, height), (2, 2))
        self.assertEqual(
            validate_visual("2x2_monochrome_gradient", pixels)["status"], "passed"
        )

    def test_color_pattern_aliases_and_ybr_422_chroma_pairs(self) -> None:
        quadrants = [
            (255, 0, 0, 255),
            (0, 255, 0, 255),
            (0, 0, 255, 255),
            (255, 255, 255, 255),
        ]
        self.assertEqual(
            validate_visual("2x2_palette_red_green_blue_white", quadrants)["status"],
            "passed",
        )
        subsampled = [
            (90, 91, 0, 255),
            (164, 165, 38, 255),
            (15, 14, 142, 255),
            (241, 240, 255, 255),
        ]
        self.assertEqual(
            validate_visual("2x2_ybr_full_422_red_green_blue_white", subsampled)["status"],
            "passed",
        )

    def test_series_observation_validates_geometry_and_gantry_capabilities(self) -> None:
        catalog = {
            "series": [{
                "id": "series-id",
                "study_instance_uid": "study",
                "series_instance_uid": "series",
                "frame_of_reference_uids": ["for"],
                "stacks": [{
                    "id": "stack-id",
                    "kind": "ordinary",
                    "warnings": [{"code": "gantry_tilt"}],
                    "frames": [
                        {
                            "virtual_index": 0,
                            "file_index": 7,
                            "frame_index": 0,
                            "source_path": "/scan/a.dcm",
                            "position_along_normal_mm": 0.0,
                        },
                        {
                            "virtual_index": 1,
                            "file_index": 9,
                            "frame_index": 0,
                            "source_path": "/scan/b.dcm",
                            "position_along_normal_mm": 5.0,
                        },
                    ],
                }],
            }],
        }
        observed = series_observation(
            catalog,
            {"index": 9},
            {
                "expected_capabilities": [
                    "sort_series_by_geometry",
                    "interpret_gantry_tilt",
                ],
                "expected_geometry": {"geometric_order_index": 2},
            },
        )
        self.assertTrue(observed["mapped"])
        self.assertTrue(observed["capabilities"]["sort_series_by_geometry"]["passed"])
        self.assertTrue(observed["capabilities"]["interpret_gantry_tilt"]["passed"])

    def test_report_validation_rejects_duplicate_result_identity(self) -> None:
        result = {
            "root": "smoke", "case_id": "classic/sc/example", "path": "x.dcm",
            "identity": {"file_sha256": "0" * 64}, "execution_safety": "safe",
            "compatibility": "full_support",
        }
        report = {
            "detail_schema_version": "0.1.0", "generated_at": "now", "worklist": {},
            "viewer": {}, "run": {}, "results": [result, result],
            "summary": {"results": 2}, "artifacts": [], "validation": {},
        }
        with self.assertRaisesRegex(RuntimeError, "duplicate result"):
            validate_report(report)

    def test_json_schema_validator_enforces_required_and_strict_fields(self) -> None:
        schema = {
            "type": "object",
            "required": ["value"],
            "additionalProperties": False,
            "properties": {"value": {"type": "string", "pattern": "^[a-z]+$"}},
        }
        validate_json_schema({"value": "valid"}, schema)
        with self.assertRaisesRegex(RuntimeError, "pattern violation"):
            validate_json_schema({"value": "INVALID"}, schema)
        with self.assertRaisesRegex(RuntimeError, "additional-field violation"):
            validate_json_schema({"value": "valid", "extra": True}, schema)

    def test_normalization_removes_timings_and_run_identity(self) -> None:
        report = {
            "detail_schema_version": "0.1.0",
            "worklist": {"content_sha256": "a" * 64},
            "viewer": {"sha256": "b" * 64},
            "run": {"selection": "smoke", "started_at": "variable"},
            "results": [{"case_id": "x", "timings_ms": {"total": 2}, "http": {"info": {"elapsed_ms": 1, "status": 200, "path": "/api/file/17/info"}}}],
            "summary": {"results": 1},
        }
        normalized = normalized_report(report)
        self.assertNotIn("timings_ms", normalized["results"][0])
        self.assertNotIn("elapsed_ms", normalized["results"][0]["http"]["info"])
        self.assertEqual(
            normalized["results"][0]["http"]["info"]["path"],
            "/api/file/{mapped}/info",
        )
        self.assertNotIn("started_at", normalized)


if __name__ == "__main__":
    unittest.main()
