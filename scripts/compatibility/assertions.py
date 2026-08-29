"""Concrete compatibility assertion registry and suite-capability routing."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class AssertionSpec:
    dimension: str
    evidence_keys: tuple[str, ...]
    description: str


def _spec(dimension: str, keys: str, description: str) -> AssertionSpec:
    return AssertionSpec(dimension, tuple(filter(None, keys.split("|"))), description)


ASSERTIONS = {
    "discovery_identity": _spec("discovery", "mapped_after_scan|file_info", "Path and SOP identity map to the discovered file."),
    "metadata_exact": _spec("metadata", "tags", "Manifest-declared metadata values are compared to typed tag evidence."),
    "raw_lossless_all_frames": _spec("pixels", "raw_headers|lossless_frame_hashes", "Every lossless frame and raw organization header matches its oracle."),
    "lossy_numeric_thresholds": _spec("pixels", "lossy_metrics", "Maximum error and RMSE meet manifest thresholds."),
    "display_normalized_pixels": _spec("presentation", "png_dimensions|normalized_display_hash", "Decoded PNG dimensions and normalized pixels match the display oracle."),
    "presentation_pipeline": _spec("presentation", "presentation_checks", "Rescale, LUT, window, inversion, padding, overlay, shutter, color, and parity checks run when applicable."),
    "frame_navigation": _spec("navigation", "frame_access", "First, middle, last, and deterministic random frames are reachable."),
    "series_geometry": _spec("navigation", "series_navigation", "Ordering, geometry warnings, and concatenations match manifest evidence."),
    "reference_closure": _spec("navigation", "references", "SOP, frame, and segment references resolve exactly when targets are present."),
    "cache_miss_hit": _spec("robustness", "display_cache|raw_cache", "Identical frame requests produce MISS then HIT."),
    "server_recovers_after_error": _spec("robustness", "recovery_after_error", "A healthy request succeeds after an intentional error."),
    "controlled_unsupported_error": _spec("unsupported", "controlled_unsupported", "The endpoint returns the policy status and stable JSON error."),
    "negative_bounded_outcome": _spec("robustness", "negative_outcome", "Malformed input finishes within bounds with a suite-permitted outcome."),
    "stress_bounded_execution": _spec("robustness", "stress_execution", "Stress scenario completes or fails within its declared envelope."),
    "stress_resource_measurements": _spec("robustness", "resource_measurements", "Startup, latency, RSS, cache, concurrency, and shutdown observations are recorded."),
    "fuzz_qualification_bounded": _spec("robustness", "fuzz_qualification", "Payload-free qualification declares bounds and no unacceptable outcomes."),
    "no_misleading_renderer": _spec("unsupported", "renderer_absent", "Metadata-only objects do not advertise an image renderer."),
    "segmentation_context": _spec("semantic", "segmentation_context", "Segment identity, coding, algorithm, and color are exact."),
    "segment_reference_closure": _spec("semantic", "segment_reference_closure", "Segment, frame, and source references close exactly."),
    "segmentation_overlay_eligibility": _spec("semantic", "segmentation_overlay", "Overlay is gated by validated references and geometry."),
    "parametric_map_context": _spec("semantic", "parametric_context", "Stored numeric layout, derivation, quantity, and units are exact."),
    "rwvm_mapping": _spec("semantic", "rwvm", "Only explicit compatible Real World Value Mapping is applied."),
    "stored_mapped_identity": _spec("semantic", "stored_mapped", "The UI distinguishes stored from mapped values."),
    "rt_dose_context": _spec("semantic", "dose_context", "Dose units, type, summation, geometry, and references are exact."),
    "dose_grid_scaling_metadata": _spec("semantic", "dose_scaling", "Dose Grid Scaling and declared mapped bounds are exact."),
    "dose_overlay_eligibility": _spec("semantic", "dose_overlay", "Dose overlay requires compatible frame of reference and geometry."),
    "semantic_context_preserves_raw": _spec("semantic", "semantic_raw_identity", "Fetching semantic context does not change raw stored pixels."),
    "wsi_tile_position": _spec("navigation", "wsi_position", "Selected frame position, optical path, focal plane, and level match metadata."),
    "wsi_minimap_metadata": _spec("presentation", "wsi_minimap", "Minimap rectangle is computed without decoding neighboring tiles."),
}


CAPABILITY_GROUPS = {
    "discovery_identity": {"open_file"},
    "metadata_exact": {"read_metadata", "show_unsupported_but_recognized"},
    "raw_lossless_all_frames": {"decode_native_pixels", "decode_rle_lossless_pixels", "decode_jpeg_ls_lossless_pixels", "decode_jpeg_xl_lossless_pixels", "decode_jpeg_2000_lossless_pixels", "decode_jpeg_lossless_process_14_pixels", "decode_jpeg_lossless_sv1_pixels", "unpack_native_bit_packed_pixels", "render_native_pixels"},
    "lossy_numeric_thresholds": {"decode_jpeg_baseline_pixels", "decode_jpeg_xl_lossy_pixels", "decode_htj2k_lossy_pixels"},
    "controlled_unsupported_error": {"decode_htj2k_lossless_pixels"},
    "display_normalized_pixels": {"render_grayscale", "render_color", "render_palette_color", "render_float_pixels", "render_double_float_pixels"},
    "presentation_pipeline": {"apply_window", "apply_modality_rescale", "apply_modality_lut", "apply_voi_lut", "apply_rescale", "read_overlay_plane", "apply_display_shutter", "color_manage_icc_profile", "apply_icc_profile"},
    "frame_navigation": {"navigate_multiframe", "parse_multiframe_functional_groups"},
    "series_geometry": {"sort_series_by_geometry", "interpret_pixel_geometry", "interpret_gantry_tilt", "organize_series_by_study_and_frame_of_reference"},
    "reference_closure": {"resolve_references", "resolve_frame_references"},
    "wsi_tile_position": {"reconstruct_total_pixel_matrix", "reconstruct_sparse_total_pixel_matrix", "reconstruct_optical_path_matrices", "reconstruct_wsi_pyramid"},
    "segmentation_context": {"parse_segmentation", "reconstruct_wsi_tile_segmentation"},
    "rwvm_mapping": {"apply_real_world_value_mapping", "read_real_world_value_mapping"},
    "parametric_map_context": {"read_image_measurement"},
    "no_misleading_renderer": {"read_structured_report", "parse_structured_report", "interpret_tid1500_measurements", "parse_scoord3d", "render_spatial_annotation", "read_spatial_registration", "apply_rigid_transform", "fuse_registered_images", "read_deformable_spatial_registration", "apply_deformation_field", "resample_registered_image", "apply_color_presentation_state", "apply_displayed_area", "apply_advanced_blending_presentation_state", "render_true_color_blend", "apply_blending_presentation_state", "render_palette_color_blend", "read_waveform", "display_twelve_lead_ecg", "display_general_ecg", "apply_presentation_state", "read_key_object_selection", "read_rt_structure_set", "read_rt_plan", "read_rt_radiation", "read_rt_radiation_set", "read_rt_image", "extract_encapsulated_document", "parse_binary_stl"},
    "rt_dose_context": {"read_rt_dose_grid"},
    "series_geometry": {"sort_series_by_geometry", "interpret_pixel_geometry", "interpret_gantry_tilt", "organize_series_by_study_and_frame_of_reference", "interpret_projection_geometry", "interpret_nm_dimensions", "interpret_pet_activity", "interpret_frame_time"},
}

CAPABILITY_ASSERTIONS: dict[str, str] = {}
for assertion_id, capabilities in CAPABILITY_GROUPS.items():
    for capability in capabilities:
        if capability in CAPABILITY_ASSERTIONS:
            raise RuntimeError(f"capability routed twice: {capability}")
        CAPABILITY_ASSERTIONS[capability] = assertion_id


def assertion_passed(value: Any) -> bool:
    if value is True:
        return True
    if isinstance(value, dict):
        return value.get("passed") is True or value.get("status") in {"passed", "not_applicable"}
    return False


def evaluate(assertion_id: str, evidence: dict[str, Any]) -> dict[str, Any]:
    spec = ASSERTIONS[assertion_id]
    observed = {key: evidence.get(key) for key in spec.evidence_keys}
    available = [value for value in observed.values() if value is not None]
    if not available:
        status = "failed"
    else:
        status = "passed" if all(assertion_passed(value) for value in available) else "failed"
    return {"assertion": assertion_id, "dimension": spec.dimension, "status": status, "evidence": observed}


def validate_registry(policy: dict[str, Any], manifest_rows: list[dict[str, Any]]) -> None:
    referenced = {
        assertion
        for rule in policy["rules"]
        for key in (
            "required_assertions",
            "conditional_assertions",
            "semantic_context_assertions",
        )
        for assertion in rule.get(key, [])
    }
    missing_assertions = referenced - set(ASSERTIONS)
    if missing_assertions:
        raise ValueError(f"policy references unknown assertions: {sorted(missing_assertions)}")
    capabilities = {capability for row in manifest_rows for capability in row.get("expected_capabilities", [])}
    missing_capabilities = capabilities - set(CAPABILITY_ASSERTIONS)
    if missing_capabilities:
        raise ValueError(f"manifest capabilities lack concrete assertion routing: {sorted(missing_capabilities)}")
