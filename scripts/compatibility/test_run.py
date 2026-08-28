from __future__ import annotations

import hashlib
import struct
import subprocess
import sys
import unittest
import zlib
from pathlib import Path

from scripts.compatibility.run import (
    PROBED_CAPABILITIES,
    normalized_report,
    icc_profile_observation,
    overlay_display_observation,
    pixel_geometry_observation,
    png_pixels,
    reference_observation,
    series_observation,
    shutter_display_observation,
    validate_json_schema,
    validate_report,
    validate_visual,
    unprobed_capabilities,
)


def grayscale_png(
    values: bytes, width: int, height: int, icc_profile: bytes | None = None
) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    rows = b"".join(b"\x00" + values[row * width : (row + 1) * width] for row in range(height))
    iccp = b""
    if icc_profile is not None:
        iccp = chunk(b"iCCP", b"DICOM ICC\0\0" + zlib.compress(icc_profile))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + iccp
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


class RunnerTests(unittest.TestCase):
    def test_overlay_capability_requires_exact_declared_display_pixels(self) -> None:
        expected = {
            "image": {"rows": 2, "columns": 2},
            "expected_semantics": {"overlay_pattern": "2x2_diagonal_overlay"},
        }
        pixels = [
            (255, 255, 255, 255),
            (85, 85, 85, 255),
            (170, 170, 170, 255),
            (255, 255, 255, 255),
        ]
        observed = overlay_display_observation(expected, pixels)
        self.assertTrue(observed["exact_evidence"])
        self.assertEqual(observed["expected_white_indices"], [0, 3])
        self.assertNotIn(
            "read_overlay_plane",
            unprobed_capabilities(
                ["read_overlay_plane"], {"overlay_display": observed}
            ),
        )

        mismatch = overlay_display_observation(expected, pixels[:3] + [(0, 0, 0, 255)])
        self.assertFalse(mismatch["exact_evidence"])
        self.assertIn(
            "read_overlay_plane",
            unprobed_capabilities(
                ["read_overlay_plane"], {"overlay_display": mismatch}
            ),
        )

    def test_full_frame_shutter_records_non_regression_without_claiming_application(self) -> None:
        expected = {
            "image": {"rows": 2, "columns": 2},
            "expected_semantics": {
                "display_shutter": {
                    "shape": "RECTANGULAR",
                    "left_vertical_edge": "1",
                    "right_vertical_edge": "2",
                    "upper_horizontal_edge": "1",
                    "lower_horizontal_edge": "2",
                }
            },
        }
        pixels = [(value, value, value, 255) for value in (0, 85, 170, 255)]
        observed = shutter_display_observation(expected, pixels)
        self.assertTrue(observed["opening_covers_full_frame"])
        self.assertTrue(observed["non_regression_passed"])
        self.assertFalse(observed["exact_evidence"])
        self.assertIn("cannot prove outside-opening replacement", observed["caveat"])
        self.assertIn(
            "apply_display_shutter",
            unprobed_capabilities(
                ["apply_display_shutter"], {"display_shutter": observed}
            ),
        )

    def test_icc_probe_hashes_png_profile_without_claiming_color_transform(self) -> None:
        profile = b"synthetic ICC profile bytes"
        expected = {
            "expected_icc_profile": {
                "profile_sha256": hashlib.sha256(profile).hexdigest(),
                "profile_size_bytes": len(profile),
            }
        }
        observed = icc_profile_observation(
            expected, grayscale_png(bytes((0, 85, 170, 255)), 2, 2, profile)
        )
        self.assertTrue(observed["profile_present"])
        self.assertTrue(observed["exact_evidence"])
        self.assertFalse(observed["numeric_transform_verified"])
        self.assertFalse(observed["path_specific_mapping_verified"])
        self.assertIn(
            "apply_icc_profile",
            unprobed_capabilities(
                ["apply_icc_profile"], {"icc_profile": observed}
            ),
        )

    def test_reference_observation_requires_exact_identity_and_local_frames(self) -> None:
        expected = [
            {
                "relationship": "source_image",
                "sop_class_uid": "1.2.class",
                "sop_instance_uid": "1.2.instance",
                "series_instance_uid": "1.2.series",
                "frame_numbers": [1, 4],
                "source_path": "source/instance.dcm",
            }
        ]
        payload = {
            "references": [
                {
                    "relationship": "source_image",
                    "target": {
                        "sop_class_uid": "1.2.class",
                        "sop_instance_uid": "1.2.instance",
                        "series_instance_uid": "1.2.series",
                        "frame_numbers": [1, 4],
                        "segment_numbers": [],
                    },
                    "matches": [
                        {
                            "file_index": 9,
                            "path": "/corpus/extended/source/instance.dcm",
                            "sop_instance_uid": "1.2.instance",
                            "frame_indices": [0, 3],
                        }
                    ],
                }
            ]
        }
        observed = reference_observation(payload, expected)
        self.assertTrue(observed["identities_match"])
        self.assertTrue(observed["all_resolved"])
        self.assertTrue(observed["exact_evidence"])
        self.assertNotIn(
            "resolve_references",
            unprobed_capabilities(
                ["resolve_references"], {"references": observed}
            ),
        )

        payload["references"][0]["matches"][0]["frame_indices"] = [0]
        mismatch = reference_observation(payload, expected)
        self.assertFalse(mismatch["exact_evidence"])
        self.assertIn(
            "resolve_references",
            unprobed_capabilities(
                ["resolve_references"], {"references": mismatch}
            ),
        )

    def test_pixel_geometry_requires_exact_api_evidence_without_claiming_ui_rendering(self) -> None:
        expected = {
            "expected_nonsquare_spacing": {
                "pixel_spacing": {
                    "row_spacing_mm": 0.6,
                    "column_spacing_mm": 0.3,
                },
                "pixel_aspect_ratio": None,
            },
        }
        observed = pixel_geometry_observation(
            {"pixel_aspect_ratio": 2.0}, expected
        )

        self.assertEqual(observed["source"], "pixel_spacing")
        self.assertEqual(observed["expected_row_to_column_ratio"], 2.0)
        self.assertEqual(observed["observed_row_to_column_ratio"], 2.0)
        self.assertTrue(observed["exact_evidence"])
        self.assertTrue(observed["passed"])
        self.assertEqual(observed["evidence_scope"], "api_files_metadata")
        self.assertFalse(observed["ui_rendering_verified"])
        self.assertNotIn(
            "interpret_pixel_geometry",
            unprobed_capabilities(
                ["interpret_pixel_geometry"], {"pixel_geometry": observed}
            ),
        )

    def test_pixel_aspect_ratio_manifest_variant_uses_vertical_horizontal_extent(self) -> None:
        expected = {
            "expected_nonsquare_spacing": {
                "pixel_spacing": None,
                "pixel_aspect_ratio": {
                    "vertical_extent": 2,
                    "horizontal_extent": 1,
                },
            },
        }
        observed = pixel_geometry_observation(
            {"pixel_aspect_ratio": 2.0}, expected
        )

        self.assertEqual(observed["source"], "pixel_aspect_ratio")
        self.assertTrue(observed["exact_evidence"])

    def test_pixel_geometry_remains_unprobed_without_an_exact_api_ratio(self) -> None:
        expected = {
            "expected_nonsquare_spacing": {
                "pixel_spacing": {
                    "row_spacing_mm": 0.6,
                    "column_spacing_mm": 0.3,
                },
            },
        }
        observed = pixel_geometry_observation(
            {"pixel_aspect_ratio": None}, expected
        )

        self.assertFalse(observed["exact_evidence"])
        self.assertFalse(observed["passed"])
        self.assertIn(
            "interpret_pixel_geometry",
            unprobed_capabilities(
                ["interpret_pixel_geometry"], {"pixel_geometry": observed}
            ),
        )

        mismatch = pixel_geometry_observation(
            {"pixel_aspect_ratio": 1.0}, expected
        )
        self.assertFalse(mismatch["exact_evidence"])
        self.assertIn(
            "interpret_pixel_geometry",
            unprobed_capabilities(
                ["interpret_pixel_geometry"], {"pixel_geometry": mismatch}
            ),
        )

    def test_rle_capability_is_backed_by_display_and_lossless_raw_probes(self) -> None:
        self.assertIn("decode_rle_lossless_pixels", PROBED_CAPABILITIES)
        self.assertIn("render_color", PROBED_CAPABILITIES)
        self.assertIn("render_palette_color", PROBED_CAPABILITIES)

    def test_grayscale_capabilities_are_backed_by_display_and_raw_probes(self) -> None:
        self.assertTrue(
            {
                "apply_modality_lut",
                "apply_modality_rescale",
                "apply_rescale",
                "apply_voi_lut",
                "apply_window",
                "decode_jpeg_2000_lossless_pixels",
                "decode_jpeg_baseline_pixels",
                "decode_jpeg_ls_lossless_pixels",
                "decode_jpeg_xl_lossless_pixels",
                "decode_native_pixels",
                "render_grayscale",
            }.issubset(PROBED_CAPABILITIES)
        )

    def test_extended_native_numeric_patterns_are_automated(self) -> None:
        checkerboard = [
            (255, 255, 255, 255),
            (0, 0, 0, 255),
            (255, 255, 255, 255),
            (0, 0, 0, 255),
            (255, 255, 255, 255),
            (0, 0, 0, 255),
            (255, 255, 255, 255),
            (0, 0, 0, 255),
            (255, 255, 255, 255),
        ]
        self.assertEqual(
            validate_visual(
                "3x3x2_continuous_lsb_first_checkerboards", checkerboard
            )["status"],
            "passed",
        )
        gradient = [
            (0, 0, 0, 255),
            (1, 1, 1, 255),
            (128, 128, 128, 255),
            (255, 255, 255, 255),
        ]
        self.assertEqual(
            validate_visual("2x2_monochrome_u32_unsigned_boundaries", gradient)[
                "status"
            ],
            "passed",
        )

    def test_named_monochrome_gradient_families_are_automated(self) -> None:
        ascending = [
            (0, 0, 0, 255),
            (85, 85, 85, 255),
            (170, 170, 170, 255),
            (255, 255, 255, 255),
        ]
        descending = list(reversed(ascending))
        self.assertEqual(
            validate_visual("2x2_signed_ct_hu_gradient", ascending)["status"],
            "passed",
        )
        self.assertEqual(
            validate_visual(
                "2x2_inverse_monochrome_i16_rle_lossless_gradient", descending
            )["status"],
            "passed",
        )
        self.assertEqual(
            validate_visual("2x2_signed_ct_hu_gradient", descending)["status"],
            "failed",
        )

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

    def test_normalization_removes_timings_run_identity_and_registry_indices(self) -> None:
        def report(registry_index: int, series_hash: str) -> dict:
            return {
                "detail_schema_version": "0.1.0",
                "worklist": {"content_sha256": "a" * 64},
                "viewer": {"sha256": "b" * 64},
                "run": {"selection": "smoke", "started_at": "variable"},
                "results": [
                    {
                        "case_id": "x",
                        "timings_ms": {"total": 2},
                        "observations": {
                            "references": {
                                "matches": [
                                    {
                                        "file_index": registry_index,
                                        "path": "/corpus/source.dcm",
                                        "sop_instance_uid": "1.2.3",
                                    }
                                ]
                            }
                        },
                        "http": {
                            "info": {
                                "elapsed_ms": 1,
                                "status": 200,
                                "path": f"/api/file/{registry_index}/info",
                                "body_sha256": "c" * 64,
                            },
                            "series_catalog": {
                                "elapsed_ms": 2,
                                "status": 200,
                                "path": "/api/series",
                                "body_sha256": series_hash,
                                "size_bytes": 100 + registry_index,
                            },
                            "references": {
                                "elapsed_ms": 3,
                                "status": 200,
                                "path": f"/api/file/{registry_index}/references",
                                "body_sha256": series_hash,
                                "size_bytes": 200 + registry_index,
                            },
                        },
                    },
                ],
                "summary": {"results": 1},
            }
        normalized = normalized_report(report(17, "d" * 64))
        self.assertNotIn("timings_ms", normalized["results"][0])
        self.assertNotIn("elapsed_ms", normalized["results"][0]["http"]["info"])
        self.assertEqual(
            normalized["results"][0]["http"]["info"]["path"],
            "/api/file/{mapped}/info",
        )
        self.assertNotIn(
            "file_index",
            normalized["results"][0]["observations"]["references"]["matches"][0],
        )
        self.assertEqual(
            normalized["results"][0]["observations"]["references"]["matches"][0][
                "sop_instance_uid"
            ],
            "1.2.3",
        )
        self.assertNotIn(
            "body_sha256", normalized["results"][0]["http"]["series_catalog"]
        )
        self.assertNotIn(
            "size_bytes", normalized["results"][0]["http"]["series_catalog"]
        )
        self.assertNotIn(
            "body_sha256", normalized["results"][0]["http"]["references"]
        )
        self.assertNotIn("size_bytes", normalized["results"][0]["http"]["references"])
        self.assertIn("body_sha256", normalized["results"][0]["http"]["info"])
        self.assertNotIn("started_at", normalized)
        self.assertEqual(
            normalized,
            normalized_report(report(104, "e" * 64)),
        )


if __name__ == "__main__":
    unittest.main()
