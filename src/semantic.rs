//! Conservative semantic metadata for objects whose pixels have a domain meaning.
//!
//! This module never changes frame bytes. It reports declared mappings and only
//! marks overlays eligible when identity and patient geometry resolve uniquely.

use crate::api::contracts::{
    CodedConceptSummary, DoseGridGeometry, OverlayEligibility, ParametricMapContext,
    RealWorldValueMappingSummary, ReferenceMatchSummary, ReferenceSummary, ReferenceTargetSummary,
    RtDoseContext, SegmentFrameMapping, SegmentSummary, SegmentationContext, SemanticContext,
    SemanticContextResponse,
};
use crate::object_kind::{classify_sop_class, ObjectKind};
use crate::references::{self, ReferenceCandidate, ReferenceRelationship, ResolvedReferenceEdge};
use crate::types::FileEntry;
use anyhow::{Context, Result};
use dicom_core::Tag;
use dicom_dictionary_std::{tags, StandardDataDictionary};
use dicom_object::{InMemDicomObject, OpenFileOptions};

const MAX_SEQUENCE_ITEMS: usize = 4_096;
const MAX_LUT_VALUES: usize = 4_096;

pub fn semantic_context(
    source: &FileEntry,
    files: &[FileEntry],
) -> Result<SemanticContextResponse> {
    let object = OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .open_file(&source.path)
        .with_context(|| {
            format!(
                "failed to open semantic metadata: {}",
                source.path.display()
            )
        })?;
    let candidates = files
        .iter()
        .map(|file| ReferenceCandidate {
            file_index: file.index,
            path: file.path.clone(),
            sop_class_uid: file.sop_class_uid.clone(),
            sop_instance_uid: file.sop_instance_uid.clone(),
            series_instance_uid: file.series_instance_uid.clone(),
            frame_count: file.frame_count,
        })
        .collect::<Vec<_>>();
    let edges = references::extract_reference_edges_from_object(&object);
    let resolved = references::resolve_reference_edges(&edges, &candidates);

    let context = match classify_sop_class(&source.sop_class_uid) {
        ObjectKind::Segmentation => {
            SemanticContext::Segmentation(segmentation_context(source, &object, files, &resolved))
        }
        ObjectKind::ParametricMap => {
            SemanticContext::ParametricMap(parametric_map_context(&object, files, &resolved))
        }
        ObjectKind::RadiationTherapy if source.sop_class_uid == "1.2.840.10008.5.1.4.1.1.481.2" => {
            SemanticContext::RtDose(Box::new(rt_dose_context(source, &object, files, &resolved)))
        }
        _ => SemanticContext::NotApplicable {
            reason: "semantic context is only defined for SEG, Parametric Map, and RT Dose"
                .to_string(),
        },
    };
    Ok(SemanticContextResponse {
        source_file_index: source.index,
        default_mode: "pixel_preview".to_string(),
        pixel_preview_preserves_stored_values: true,
        context,
    })
}

fn segmentation_context(
    source: &FileEntry,
    object: &InMemDicomObject<StandardDataDictionary>,
    files: &[FileEntry],
    resolved: &[ResolvedReferenceEdge],
) -> SegmentationContext {
    let segments = sequence_items(object, tags::SEGMENT_SEQUENCE)
        .into_iter()
        .take(MAX_SEQUENCE_ITEMS)
        .filter_map(|item| {
            Some(SegmentSummary {
                number: read_number::<u16>(item, tags::SEGMENT_NUMBER)?,
                label: read_string(item, tags::SEGMENT_LABEL),
                description: read_string(item, tags::SEGMENT_DESCRIPTION),
                property_category: read_code(item, tags::SEGMENTED_PROPERTY_CATEGORY_CODE_SEQUENCE),
                property_type: read_code(item, tags::SEGMENTED_PROPERTY_TYPE_CODE_SEQUENCE),
                algorithm_type: read_string(item, tags::SEGMENT_ALGORITHM_TYPE),
                algorithm_name: read_string(item, tags::SEGMENT_ALGORITHM_NAME),
                recommended_display_cielab: optional_numbers(
                    item,
                    tags::RECOMMENDED_DISPLAY_CIE_LAB_VALUE,
                ),
                recommended_display_grayscale: read_number(
                    item,
                    tags::RECOMMENDED_DISPLAY_GRAYSCALE_VALUE,
                ),
            })
        })
        .collect::<Vec<_>>();

    let mut frame_mappings = Vec::new();
    let shared_group = sequence_items(object, tags::SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        .into_iter()
        .next();
    for (frame_index, frame_group) in
        sequence_items(object, tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            .into_iter()
            .take(source.frame_count as usize)
            .enumerate()
    {
        let segment_number = referenced_segment_number(frame_group, shared_group);
        let source_item = sequence_items(frame_group, tags::DERIVATION_IMAGE_SEQUENCE)
            .into_iter()
            .flat_map(|item| sequence_items(item, tags::SOURCE_IMAGE_SEQUENCE))
            .next();
        let source_sop_instance_uid =
            source_item.and_then(|item| read_string(item, tags::REFERENCED_SOP_INSTANCE_UID));
        let source_frame_numbers = source_item
            .map(|item| read_numbers(item, tags::REFERENCED_FRAME_NUMBER))
            .unwrap_or_default();
        let source_file_indices = source_sop_instance_uid
            .as_ref()
            .map(|uid| {
                files
                    .iter()
                    .filter(|file| &file.sop_instance_uid == uid)
                    .map(|file| file.index)
                    .collect()
            })
            .unwrap_or_default();
        frame_mappings.push(SegmentFrameMapping {
            frame_index: frame_index as u32,
            segment_number,
            source_sop_instance_uid,
            source_frame_numbers,
            source_file_indices,
        });
    }

    let declared_segments = segments
        .iter()
        .map(|segment| segment.number)
        .collect::<Vec<_>>();
    let segment_closure_valid = frame_mappings.iter().all(|mapping| {
        mapping
            .segment_number
            .is_some_and(|number| declared_segments.contains(&number))
    });
    let overlay = segmentation_overlay(
        source,
        files,
        resolved,
        &frame_mappings,
        segment_closure_valid,
    );
    SegmentationContext {
        segmentation_type: read_string(object, tags::SEGMENTATION_TYPE),
        segmentation_fractional_type: read_string(object, tags::SEGMENTATION_FRACTIONAL_TYPE),
        maximum_fractional_value: read_number(object, tags::MAXIMUM_FRACTIONAL_VALUE),
        segments,
        frame_mappings,
        references: resolved.iter().map(reference_summary).collect(),
        overlay,
    }
}

fn referenced_segment_number(
    frame_group: &InMemDicomObject<StandardDataDictionary>,
    shared_group: Option<&InMemDicomObject<StandardDataDictionary>>,
) -> Option<u16> {
    [Some(frame_group), shared_group]
        .into_iter()
        .flatten()
        .find_map(|group| {
            sequence_items(group, tags::SEGMENT_IDENTIFICATION_SEQUENCE)
                .first()
                .and_then(|item| read_number(item, tags::REFERENCED_SEGMENT_NUMBER))
        })
}

fn segmentation_overlay(
    source: &FileEntry,
    files: &[FileEntry],
    resolved: &[ResolvedReferenceEdge],
    frame_mappings: &[SegmentFrameMapping],
    segment_closure_valid: bool,
) -> OverlayEligibility {
    if !segment_closure_valid {
        return ineligible("a frame references a missing or undeclared segment number");
    }
    if frame_mappings.len() != source.frame_count as usize || frame_mappings.is_empty() {
        return ineligible("per-frame segment/source mapping is incomplete");
    }
    if frame_mappings
        .iter()
        .any(|mapping| mapping.source_file_indices.len() != 1)
    {
        return ineligible("a source instance is missing or resolves ambiguously");
    }
    let source_indices = frame_mappings
        .iter()
        .flat_map(|mapping| mapping.source_file_indices.iter().copied())
        .collect::<Vec<_>>();
    let Some(&target_index) = source_indices.first() else {
        return ineligible("no frame-level source image mapping is declared");
    };
    if source_indices.iter().any(|index| *index != target_index) {
        return ineligible("segmentation frames map to multiple source instances");
    }
    let Some(target) = files.iter().find(|file| file.index == target_index) else {
        return ineligible("the mapped source image is not available");
    };
    if !patient_geometry_compatible(source, target) {
        return ineligible("frame of reference or patient geometry is incompatible");
    }
    if !resolved.iter().any(|edge| {
        matches!(
            edge.relationship,
            ReferenceRelationship::SourceImage | ReferenceRelationship::SourceImageForSegmentation
        ) && edge
            .matches
            .iter()
            .any(|candidate| candidate.file_index == target_index)
    }) {
        return ineligible("frame mapping is not closed by a declared source reference");
    }
    OverlayEligibility {
        eligible: true,
        reason: "declared source identity and patient geometry are uniquely validated".to_string(),
        source_file_index: Some(target_index),
    }
}

fn parametric_map_context(
    object: &InMemDicomObject<StandardDataDictionary>,
    files: &[FileEntry],
    resolved: &[ResolvedReferenceEdge],
) -> ParametricMapContext {
    let mut mappings = Vec::new();
    collect_rwvm_mappings(object, "embedded", None, &mut mappings);
    let mut warnings = Vec::new();
    for reference in referenced_rwvm_instances(object) {
        let matches = files
            .iter()
            .filter(|file| file.sop_instance_uid == reference)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [file] => match OpenFileOptions::new()
                .read_until(tags::PIXEL_DATA)
                .open_file(&file.path)
            {
                Ok(mapping_object) => {
                    let before = mappings.len();
                    collect_rwvm_mappings(
                        &mapping_object,
                        "referenced",
                        Some(reference.as_str()),
                        &mut mappings,
                    );
                    if mappings.len() == before {
                        warnings.push(format!(
                            "referenced RWVM {reference} contains no usable mapping"
                        ));
                    }
                }
                Err(_) => warnings.push(format!("referenced RWVM {reference} could not be read")),
            },
            [] => warnings.push(format!("referenced RWVM {reference} is missing")),
            _ => warnings.push(format!("referenced RWVM {reference} resolves ambiguously")),
        }
    }
    let mapping_status = if mappings.is_empty() {
        "unmapped"
    } else if mappings.iter().any(valid_mapping) {
        "mapping_available"
    } else {
        "incompatible_mapping"
    };
    let stored_value_type = if object.element(tags::FLOAT_PIXEL_DATA).is_ok() {
        "float32"
    } else if object.element(tags::DOUBLE_FLOAT_PIXEL_DATA).is_ok() {
        "float64"
    } else {
        "integer"
    };
    ParametricMapContext {
        stored_value_type: stored_value_type.to_string(),
        displayed_value_kind: if mapping_status == "mapping_available" {
            "mapped"
        } else {
            "stored"
        }
        .to_string(),
        mappings,
        mapping_status: mapping_status.to_string(),
        source_references: resolved.iter().map(reference_summary).collect(),
        warnings,
    }
}

fn rt_dose_context(
    source: &FileEntry,
    object: &InMemDicomObject<StandardDataDictionary>,
    files: &[FileEntry],
    resolved: &[ResolvedReferenceEdge],
) -> RtDoseContext {
    let scaling = read_number::<f64>(object, tags::DOSE_GRID_SCALING)
        .filter(|value| value.is_finite() && *value > 0.0);
    let geometry = DoseGridGeometry {
        frame_of_reference_uid: read_string(object, tags::FRAME_OF_REFERENCE_UID),
        image_position_patient: fixed_numbers(object, tags::IMAGE_POSITION_PATIENT),
        image_orientation_patient: fixed_numbers(object, tags::IMAGE_ORIENTATION_PATIENT),
        pixel_spacing: fixed_numbers(object, tags::PIXEL_SPACING),
        grid_frame_offsets: read_numbers(object, tags::GRID_FRAME_OFFSET_VECTOR),
    };
    let overlay = rt_dose_overlay(source, files, resolved);
    RtDoseContext {
        dose_grid_scaling: scaling,
        scaling_status: if scaling.is_some() { "available" } else { "missing_or_malformed" }
            .to_string(),
        displayed_value_kind: if scaling.is_some() { "mapped" } else { "stored" }.to_string(),
        dose_units: read_string(object, tags::DOSE_UNITS),
        dose_type: read_string(object, tags::DOSE_TYPE),
        dose_summation_type: read_string(object, tags::DOSE_SUMMATION_TYPE),
        geometry,
        references: resolved.iter().map(reference_summary).collect(),
        overlay,
        clinical_use_warning:
            "Semantic context does not establish prescription correctness or clinical acceptability."
                .to_string(),
    }
}

fn rt_dose_overlay(
    source: &FileEntry,
    files: &[FileEntry],
    resolved: &[ResolvedReferenceEdge],
) -> OverlayEligibility {
    let matches = resolved
        .iter()
        .filter(|edge| edge.relationship == ReferenceRelationship::SourceImage)
        .flat_map(|edge| edge.matches.iter())
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return ineligible(if matches.is_empty() {
            "no uniquely resolved source-image reference is available"
        } else {
            "source-image reference resolves ambiguously"
        });
    }
    let target_index = matches[0].file_index;
    let Some(target) = files.iter().find(|file| file.index == target_index) else {
        return ineligible("the referenced source image is not available");
    };
    if !patient_geometry_compatible(source, target) {
        return ineligible("frame of reference or patient geometry is incompatible");
    }
    OverlayEligibility {
        eligible: true,
        reason: "declared source identity and patient geometry are uniquely validated".to_string(),
        source_file_index: Some(target_index),
    }
}

fn patient_geometry_compatible(left: &FileEntry, right: &FileEntry) -> bool {
    let left_meta = &left.series_metadata;
    let right_meta = &right.series_metadata;
    !left_meta.frame_of_reference_uid.is_empty()
        && left_meta.frame_of_reference_uid == right_meta.frame_of_reference_uid
        && left.rows == right.rows
        && left.columns == right.columns
        && near_array(
            left_meta.image_position_patient,
            right_meta.image_position_patient,
        )
        && near_array(
            left_meta.image_orientation_patient,
            right_meta.image_orientation_patient,
        )
        && near_array(
            left_meta.native_pixel.pixel_spacing,
            right_meta.native_pixel.pixel_spacing,
        )
}

fn near_array<const N: usize>(left: Option<[f64; N]>, right: Option<[f64; N]>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.iter().zip(right.iter()).all(|(left, right)| {
            left.is_finite() && right.is_finite() && (left - right).abs() <= 1e-6
        }),
        _ => false,
    }
}

fn collect_rwvm_mappings(
    object: &InMemDicomObject<StandardDataDictionary>,
    source: &str,
    source_sop_instance_uid: Option<&str>,
    output: &mut Vec<RealWorldValueMappingSummary>,
) {
    for element in object.iter() {
        let Some(items) = element.items() else {
            continue;
        };
        if element.header().tag == tags::REAL_WORLD_VALUE_MAPPING_SEQUENCE {
            output.extend(
                items
                    .iter()
                    .take(MAX_SEQUENCE_ITEMS)
                    .map(|item| rwvm_mapping(item, source, source_sop_instance_uid)),
            );
        } else {
            for item in items.iter().take(MAX_SEQUENCE_ITEMS) {
                collect_rwvm_mappings(item, source, source_sop_instance_uid, output);
            }
        }
    }
}

fn rwvm_mapping(
    item: &InMemDicomObject<StandardDataDictionary>,
    source: &str,
    source_sop_instance_uid: Option<&str>,
) -> RealWorldValueMappingSummary {
    let mut lut_data = read_numbers(item, tags::REAL_WORLD_VALUE_LUT_DATA);
    let lut_data_truncated = lut_data.len() > MAX_LUT_VALUES;
    lut_data.truncate(MAX_LUT_VALUES);
    RealWorldValueMappingSummary {
        source: source.to_string(),
        source_sop_instance_uid: source_sop_instance_uid.map(str::to_string),
        label: read_string(item, tags::LUT_LABEL),
        first_value_mapped: read_number(item, tags::REAL_WORLD_VALUE_FIRST_VALUE_MAPPED),
        last_value_mapped: read_number(item, tags::REAL_WORLD_VALUE_LAST_VALUE_MAPPED),
        slope: read_number(item, tags::REAL_WORLD_VALUE_SLOPE),
        intercept: read_number(item, tags::REAL_WORLD_VALUE_INTERCEPT),
        lut_data,
        lut_data_truncated,
        units: read_code(item, tags::MEASUREMENT_UNITS_CODE_SEQUENCE),
        quantity: read_quantity_code(item),
        derivation: read_code(item, tags::DERIVATION_CODE_SEQUENCE),
    }
}

fn read_quantity_code(
    object: &InMemDicomObject<StandardDataDictionary>,
) -> Option<CodedConceptSummary> {
    sequence_items(object, tags::QUANTITY_DEFINITION_SEQUENCE)
        .into_iter()
        .find_map(|item| {
            read_code(item, tags::CONCEPT_CODE_SEQUENCE).or_else(|| read_direct_code(item))
        })
}

fn valid_mapping(mapping: &RealWorldValueMappingSummary) -> bool {
    let range_valid = matches!(
        (mapping.first_value_mapped, mapping.last_value_mapped),
        (Some(first), Some(last)) if first.is_finite() && last.is_finite() && first <= last
    );
    let linear = matches!(
        (mapping.slope, mapping.intercept),
        (Some(slope), Some(intercept)) if slope.is_finite() && intercept.is_finite()
    );
    range_valid && (linear || !mapping.lut_data.is_empty()) && mapping.units.is_some()
}

fn referenced_rwvm_instances(object: &InMemDicomObject<StandardDataDictionary>) -> Vec<String> {
    sequence_items(
        object,
        tags::REFERENCED_REAL_WORLD_VALUE_MAPPING_INSTANCE_SEQUENCE,
    )
    .into_iter()
    .filter_map(|item| read_string(item, tags::REFERENCED_SOP_INSTANCE_UID))
    .collect()
}

fn reference_summary(edge: &ResolvedReferenceEdge) -> ReferenceSummary {
    ReferenceSummary {
        relationship: edge.relationship.as_str().to_string(),
        target: ReferenceTargetSummary {
            sop_class_uid: edge.target.sop_class_uid.clone(),
            sop_instance_uid: edge.target.sop_instance_uid.clone(),
            series_instance_uid: edge.target.series_instance_uid.clone(),
            frame_numbers: edge.target.frame_numbers.clone(),
            segment_numbers: edge.target.segment_numbers.clone(),
        },
        matches: edge
            .matches
            .iter()
            .map(|target| ReferenceMatchSummary {
                file_index: target.file_index,
                path: target.path.display().to_string(),
                sop_instance_uid: target.sop_instance_uid.clone(),
                frame_indices: target.frame_indices.clone(),
            })
            .collect(),
    }
}

fn ineligible(reason: &str) -> OverlayEligibility {
    OverlayEligibility {
        eligible: false,
        reason: reason.to_string(),
        source_file_index: None,
    }
}

fn sequence_items(
    object: &InMemDicomObject<StandardDataDictionary>,
    tag: Tag,
) -> Vec<&InMemDicomObject<StandardDataDictionary>> {
    object
        .element(tag)
        .ok()
        .and_then(|element| element.items())
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn read_code(
    object: &InMemDicomObject<StandardDataDictionary>,
    tag: Tag,
) -> Option<CodedConceptSummary> {
    let item = sequence_items(object, tag).into_iter().next()?;
    read_direct_code(item)
}

fn read_direct_code(
    item: &InMemDicomObject<StandardDataDictionary>,
) -> Option<CodedConceptSummary> {
    let value = read_string(item, tags::CODE_VALUE)
        .or_else(|| read_string(item, tags::LONG_CODE_VALUE))
        .or_else(|| read_string(item, tags::URN_CODE_VALUE))?;
    Some(CodedConceptSummary {
        value,
        scheme: read_string(item, tags::CODING_SCHEME_DESIGNATOR).unwrap_or_default(),
        meaning: read_string(item, tags::CODE_MEANING).unwrap_or_default(),
    })
}

fn read_string(object: &InMemDicomObject<StandardDataDictionary>, tag: Tag) -> Option<String> {
    let value = object.element(tag).ok()?.to_str().ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn read_number<T>(object: &InMemDicomObject<StandardDataDictionary>, tag: Tag) -> Option<T>
where
    T: std::str::FromStr,
{
    read_string(object, tag)?
        .split('\\')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn read_numbers<T>(object: &InMemDicomObject<StandardDataDictionary>, tag: Tag) -> Vec<T>
where
    T: std::str::FromStr,
{
    read_string(object, tag)
        .map(|value| {
            value
                .split('\\')
                .filter_map(|part| part.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn optional_numbers<T>(
    object: &InMemDicomObject<StandardDataDictionary>,
    tag: Tag,
) -> Option<Vec<T>>
where
    T: std::str::FromStr,
{
    let values = read_numbers(object, tag);
    (!values.is_empty()).then_some(values)
}

fn fixed_numbers<const N: usize>(
    object: &InMemDicomObject<StandardDataDictionary>,
    tag: Tag,
) -> Option<[f64; N]> {
    read_numbers::<f64>(object, tag).try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::referenced_segment_number;
    use dicom_core::{value::DataSetSequence, DataElement, PrimitiveValue, VR};
    use dicom_dictionary_std::tags;
    use dicom_object::InMemDicomObject;

    fn group(segment_number: u16) -> InMemDicomObject {
        let identification = InMemDicomObject::from_element_iter([DataElement::new(
            tags::REFERENCED_SEGMENT_NUMBER,
            VR::US,
            PrimitiveValue::U16(vec![segment_number].into()),
        )]);
        InMemDicomObject::from_element_iter([DataElement::new(
            tags::SEGMENT_IDENTIFICATION_SEQUENCE,
            VR::SQ,
            DataSetSequence::from(vec![identification]),
        )])
    }

    #[test]
    fn per_frame_segment_number_overrides_shared_functional_group() {
        let per_frame = group(2);
        let shared = group(1);
        assert_eq!(
            referenced_segment_number(&per_frame, Some(&shared)),
            Some(2)
        );
    }

    #[test]
    fn shared_segment_number_applies_when_per_frame_group_omits_it() {
        let per_frame = InMemDicomObject::new_empty();
        let shared = group(1);
        assert_eq!(
            referenced_segment_number(&per_frame, Some(&shared)),
            Some(1)
        );
    }
}
