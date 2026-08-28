use super::support;
use axum_test::TestServer;
use dcmview::server;
use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use serde_json::Value;
use std::path::Path;
use tempfile::tempdir;

const SEG_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.4";
const RT_DOSE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.2";
const CT_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2";

#[tokio::test]
async fn segmentation_context_reports_segment_closure_and_validated_overlay() {
    let dir = tempdir().expect("temp dir");
    let seg_path = dir.path().join("seg.dcm");
    let source_uid = "2.25.8001";
    let source_series_uid = "2.25.8002";

    let source_reference = reference_item(CT_STORAGE, source_uid, Some("1"));
    let referenced_series = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SERIES_INSTANCE_UID, VR::UI, source_series_uid),
        sequence(tags::REFERENCED_INSTANCE_SEQUENCE, vec![source_reference]),
    ]);
    let segment = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SEGMENT_NUMBER, VR::US, PrimitiveValue::from(1_u16)),
        DataElement::new(tags::SEGMENT_LABEL, VR::LO, "Tumor"),
        DataElement::new(tags::SEGMENT_DESCRIPTION, VR::ST, "Research region"),
        DataElement::new(tags::SEGMENT_ALGORITHM_TYPE, VR::CS, "AUTOMATIC"),
        DataElement::new(tags::SEGMENT_ALGORITHM_NAME, VR::LO, "fixture"),
        sequence(
            tags::SEGMENTED_PROPERTY_TYPE_CODE_SEQUENCE,
            vec![code("M-80003", "SRT", "Neoplasm")],
        ),
        DataElement::new(
            tags::RECOMMENDED_DISPLAY_CIE_LAB_VALUE,
            VR::US,
            PrimitiveValue::U16(vec![40_000, 20_000, 10_000].into()),
        ),
    ]);
    let segment_identification = InMemDicomObject::from_element_iter([DataElement::new(
        tags::REFERENCED_SEGMENT_NUMBER,
        VR::US,
        PrimitiveValue::from(1_u16),
    )]);
    let derivation = InMemDicomObject::from_element_iter([sequence(
        tags::SOURCE_IMAGE_SEQUENCE,
        vec![reference_item(CT_STORAGE, source_uid, Some("1"))],
    )]);
    let frame_group = InMemDicomObject::from_element_iter([
        sequence(
            tags::SEGMENT_IDENTIFICATION_SEQUENCE,
            vec![segment_identification],
        ),
        sequence(tags::DERIVATION_IMAGE_SEQUENCE, vec![derivation]),
    ]);
    let object = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SOP_CLASS_UID, VR::UI, SEG_STORAGE),
        DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.8000"),
        DataElement::new(tags::SEGMENTATION_TYPE, VR::CS, "BINARY"),
        sequence(tags::SEGMENT_SEQUENCE, vec![segment]),
        sequence(tags::REFERENCED_SERIES_SEQUENCE, vec![referenced_series]),
        sequence(
            tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
            vec![frame_group],
        ),
    ]);
    write_object(&seg_path, SEG_STORAGE, "2.25.8000", object);

    let mut seg = support::file_entry(seg_path, uids::EXPLICIT_VR_LITTLE_ENDIAN, 1);
    seg.sop_class_uid = SEG_STORAGE.to_string();
    seg.sop_instance_uid = "2.25.8000".to_string();
    configure_geometry(&mut seg, "2.25.8099");
    let mut source = support::file_entry(
        dir.path().join("source.dcm"),
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        1,
    );
    source.sop_class_uid = CT_STORAGE.to_string();
    source.sop_instance_uid = source_uid.to_string();
    source.series_instance_uid = source_series_uid.to_string();
    configure_geometry(&mut source, "2.25.8099");

    let response: Value = TestServer::new(server::router(support::app_state(vec![seg, source])))
        .get("/api/file/0/semantic-context")
        .await
        .json();
    assert_eq!(response["default_mode"], "pixel_preview");
    assert_eq!(response["pixel_preview_preserves_stored_values"], true);
    let context = &response["context"];
    assert_eq!(context["kind"], "segmentation");
    assert_eq!(context["segmentation_type"], "BINARY");
    assert_eq!(context["segments"][0]["number"], 1);
    assert_eq!(
        context["segments"][0]["property_type"]["meaning"],
        "Neoplasm"
    );
    assert_eq!(context["frame_mappings"][0]["segment_number"], 1);
    assert_eq!(
        context["frame_mappings"][0]["source_file_indices"],
        serde_json::json!([1])
    );
    assert_eq!(context["overlay"]["eligible"], true);
    assert_eq!(context["overlay"]["source_file_index"], 1);
}

#[tokio::test]
async fn parametric_map_context_exposes_explicit_mapping_without_applying_it() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("pm.dcm");
    let mapping = InMemDicomObject::from_element_iter([
        DataElement::new(tags::LUT_LABEL, VR::SH, "SUVbw"),
        DataElement::new(
            tags::REAL_WORLD_VALUE_FIRST_VALUE_MAPPED,
            VR::FD,
            PrimitiveValue::from(0.0_f64),
        ),
        DataElement::new(
            tags::REAL_WORLD_VALUE_LAST_VALUE_MAPPED,
            VR::FD,
            PrimitiveValue::from(100.0_f64),
        ),
        DataElement::new(
            tags::REAL_WORLD_VALUE_SLOPE,
            VR::FD,
            PrimitiveValue::from(0.5_f64),
        ),
        DataElement::new(
            tags::REAL_WORLD_VALUE_INTERCEPT,
            VR::FD,
            PrimitiveValue::from(-1.0_f64),
        ),
        sequence(
            tags::MEASUREMENT_UNITS_CODE_SEQUENCE,
            vec![code("g/ml", "UCUM", "grams per milliliter")],
        ),
        sequence(
            tags::QUANTITY_DEFINITION_SEQUENCE,
            vec![code("126400", "DCM", "Standardized Uptake Value")],
        ),
    ]);
    let object = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SOP_CLASS_UID, VR::UI, uids::PARAMETRIC_MAP_STORAGE),
        DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.8100"),
        sequence(tags::REAL_WORLD_VALUE_MAPPING_SEQUENCE, vec![mapping]),
        DataElement::new(
            tags::FLOAT_PIXEL_DATA,
            VR::OF,
            PrimitiveValue::F32(vec![1.0, 2.0].into()),
        ),
    ]);
    write_object(&path, uids::PARAMETRIC_MAP_STORAGE, "2.25.8100", object);
    let mut entry = support::file_entry(path, uids::EXPLICIT_VR_LITTLE_ENDIAN, 1);
    entry.sop_class_uid = uids::PARAMETRIC_MAP_STORAGE.to_string();

    let response: Value = TestServer::new(server::router(support::app_state(vec![entry])))
        .get("/api/file/0/semantic-context")
        .await
        .json();
    let context = &response["context"];
    assert_eq!(context["kind"], "parametric_map");
    assert_eq!(context["stored_value_type"], "float32");
    assert_eq!(context["displayed_value_kind"], "mapped");
    assert_eq!(context["mapping_status"], "mapping_available");
    assert_eq!(context["mappings"][0]["slope"], 0.5);
    assert_eq!(context["mappings"][0]["intercept"], -1.0);
    assert_eq!(context["mappings"][0]["units"]["scheme"], "UCUM");
}

#[tokio::test]
async fn rt_dose_context_reports_scaling_geometry_and_refuses_incompatible_overlay() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("dose.dcm");
    let target_uid = "2.25.8201";
    let object = InMemDicomObject::from_element_iter([
        DataElement::new(tags::SOP_CLASS_UID, VR::UI, RT_DOSE_STORAGE),
        DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.8200"),
        DataElement::new(tags::DOSE_GRID_SCALING, VR::DS, "0.0025"),
        DataElement::new(tags::DOSE_UNITS, VR::CS, "GY"),
        DataElement::new(tags::DOSE_TYPE, VR::CS, "PHYSICAL"),
        DataElement::new(tags::DOSE_SUMMATION_TYPE, VR::CS, "PLAN"),
        DataElement::new(tags::FRAME_OF_REFERENCE_UID, VR::UI, "2.25.8299"),
        DataElement::new(tags::IMAGE_POSITION_PATIENT, VR::DS, "0\\0\\0"),
        DataElement::new(tags::IMAGE_ORIENTATION_PATIENT, VR::DS, "1\\0\\0\\0\\1\\0"),
        DataElement::new(tags::PIXEL_SPACING, VR::DS, "1\\1"),
        DataElement::new(tags::GRID_FRAME_OFFSET_VECTOR, VR::DS, "0\\2.5\\5"),
        sequence(
            tags::REFERENCED_IMAGE_SEQUENCE,
            vec![reference_item(CT_STORAGE, target_uid, Some("1"))],
        ),
    ]);
    write_object(&path, RT_DOSE_STORAGE, "2.25.8200", object);
    let mut dose = support::file_entry(path, uids::EXPLICIT_VR_LITTLE_ENDIAN, 3);
    dose.sop_class_uid = RT_DOSE_STORAGE.to_string();
    configure_geometry(&mut dose, "2.25.8299");
    let mut target = support::file_entry(
        dir.path().join("target.dcm"),
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        1,
    );
    target.sop_instance_uid = target_uid.to_string();
    target.sop_class_uid = CT_STORAGE.to_string();
    configure_geometry(&mut target, "2.25.different");

    let response: Value = TestServer::new(server::router(support::app_state(vec![dose, target])))
        .get("/api/file/0/semantic-context")
        .await
        .json();
    let context = &response["context"];
    assert_eq!(context["kind"], "rt_dose");
    assert_eq!(context["dose_grid_scaling"], 0.0025);
    assert_eq!(context["scaling_status"], "available");
    assert_eq!(context["displayed_value_kind"], "mapped");
    assert_eq!(context["dose_units"], "GY");
    assert_eq!(
        context["geometry"]["grid_frame_offsets"],
        serde_json::json!([0.0, 2.5, 5.0])
    );
    assert_eq!(context["overlay"]["eligible"], false);
    assert!(context["overlay"]["reason"]
        .as_str()
        .expect("overlay reason")
        .contains("incompatible"));
    assert!(context["clinical_use_warning"]
        .as_str()
        .expect("warning")
        .contains("clinical acceptability"));
}

fn configure_geometry(entry: &mut dcmview::types::FileEntry, frame_of_reference_uid: &str) {
    entry.rows = 16;
    entry.columns = 16;
    entry.series_metadata.frame_of_reference_uid = frame_of_reference_uid.to_string();
    entry.series_metadata.image_position_patient = Some([0.0, 0.0, 0.0]);
    entry.series_metadata.image_orientation_patient = Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    entry.series_metadata.native_pixel.pixel_spacing = Some([1.0, 1.0]);
}

fn reference_item(
    sop_class_uid: &str,
    sop_instance_uid: &str,
    frame_number: Option<&str>,
) -> InMemDicomObject {
    let mut item = InMemDicomObject::from_element_iter([
        DataElement::new(tags::REFERENCED_SOP_CLASS_UID, VR::UI, sop_class_uid),
        DataElement::new(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, sop_instance_uid),
    ]);
    if let Some(frame_number) = frame_number {
        item.put(DataElement::new(
            tags::REFERENCED_FRAME_NUMBER,
            VR::IS,
            frame_number,
        ));
    }
    item
}

fn code(value: &str, scheme: &str, meaning: &str) -> InMemDicomObject {
    InMemDicomObject::from_element_iter([
        DataElement::new(tags::CODE_VALUE, VR::SH, value),
        DataElement::new(tags::CODING_SCHEME_DESIGNATOR, VR::SH, scheme),
        DataElement::new(tags::CODE_MEANING, VR::LO, meaning),
    ])
}

fn sequence(tag: dicom_core::Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn write_object(
    path: &Path,
    sop_class_uid: &str,
    sop_instance_uid: &str,
    object: InMemDicomObject,
) {
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(sop_class_uid)
                .media_storage_sop_instance_uid(sop_instance_uid),
        )
        .expect("build file meta")
        .write_to_file(path)
        .expect("write semantic DICOM object");
}
