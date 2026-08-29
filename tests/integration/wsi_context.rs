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

const WSI_STORAGE: &str = uids::VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE;

#[tokio::test]
async fn tiled_full_context_derives_bounded_position_optical_path_level_and_companions() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("full.dcm");
    write_object(
        &path,
        InMemDicomObject::from_element_iter([
            DataElement::new(tags::SOP_CLASS_UID, VR::UI, WSI_STORAGE),
            DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.9000"),
        ]),
    );
    let mut source = wsi_entry(path, 24_000);
    source.sop_instance_uid = "2.25.9000".to_string();
    source.rows = 1_000;
    source.columns = 1_000;
    source.series_metadata.dimension_organization_type = Some("TILED_FULL".to_string());
    source.series_metadata.total_pixel_matrix_rows = Some(100_000);
    source.series_metadata.total_pixel_matrix_columns = Some(120_000);
    source.series_metadata.number_of_optical_paths = Some(2);
    source.series_metadata.total_pixel_matrix_focal_planes = Some(1);
    source.series_metadata.optical_path_identifiers = vec!["RGB".to_string(), "IHC".to_string()];
    source.series_metadata.pyramid_uid = Some("2.25.pyramid".to_string());
    source.series_metadata.container_identifier = Some("SLIDE-1".to_string());
    source.series_metadata.image_type = vec![
        "ORIGINAL".to_string(),
        "PRIMARY".to_string(),
        "VOLUME".to_string(),
    ];

    let mut files = vec![source];
    for index in 0..70 {
        let mut companion = wsi_entry(dir.path().join(format!("companion-{index}.dcm")), 1);
        companion.sop_instance_uid = format!("2.25.91{index}");
        companion.series_metadata.container_identifier = Some("SLIDE-1".to_string());
        companion.series_metadata.image_type = vec![
            "DERIVED".to_string(),
            "PRIMARY".to_string(),
            "THUMBNAIL".to_string(),
        ];
        files.push(companion);
    }
    let response: Value = TestServer::new(server::router(support::app_state(files)))
        .get("/api/file/0/frame/12121/wsi-context")
        .await
        .json();
    assert_eq!(response["tiling_status"], "full");
    assert_eq!(response["position_source"], "dicom_tiled_full_raster");
    assert_eq!(response["positioning_status"], "positioned");
    assert_eq!(response["tile_rectangle"]["x"], 1_000);
    assert_eq!(response["tile_rectangle"]["y"], 1_000);
    assert_eq!(response["tile_row"], 1);
    assert_eq!(response["tile_column"], 1);
    assert_eq!(response["optical_path"]["index"], 1);
    assert_eq!(response["optical_path"]["identifier"], "IHC");
    assert_eq!(response["focal_plane"]["index"], 0);
    assert_eq!(response["pyramid_level"], 0);
    assert_eq!(
        response["companions"].as_array().expect("companions").len(),
        64
    );
    assert_eq!(response["companions_truncated"], true);
    assert_eq!(response["reconstruction_claimed"], false);
    assert!(response["warnings"]
        .as_array()
        .expect("warnings")
        .is_empty());
}

#[tokio::test]
async fn tiled_sparse_context_uses_selected_frame_position_optical_path_and_focal_plane() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("sparse.dcm");
    let frame_one = frame_group(1, 1, "RGB", 0.0);
    let frame_two = frame_group(257, 513, "IHC", 2.5);
    write_object(
        &path,
        InMemDicomObject::from_element_iter([
            DataElement::new(tags::SOP_CLASS_UID, VR::UI, WSI_STORAGE),
            DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.9200"),
            sequence(
                tags::PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
                vec![frame_one, frame_two],
            ),
        ]),
    );
    let mut source = wsi_entry(path, 2);
    source.rows = 256;
    source.columns = 256;
    source.series_metadata.dimension_organization_type = Some("TILED_SPARSE".to_string());
    source.series_metadata.total_pixel_matrix_rows = Some(1_024);
    source.series_metadata.total_pixel_matrix_columns = Some(2_048);
    source.series_metadata.number_of_optical_paths = Some(2);
    source.series_metadata.total_pixel_matrix_focal_planes = Some(2);
    source.series_metadata.optical_path_identifiers = vec!["RGB".to_string(), "IHC".to_string()];

    let response: Value = TestServer::new(server::router(support::app_state(vec![source])))
        .get("/api/file/0/frame/1/wsi-context")
        .await
        .json();
    assert_eq!(response["tiling_status"], "sparse");
    assert_eq!(response["position_source"], "declared_per_frame");
    assert_eq!(response["tile_rectangle"]["x"], 512);
    assert_eq!(response["tile_rectangle"]["y"], 256);
    assert_eq!(response["tile_row"], 1);
    assert_eq!(response["tile_column"], 2);
    assert_eq!(response["optical_path"]["index"], 1);
    assert_eq!(response["optical_path"]["identifier"], "IHC");
    assert_eq!(response["focal_plane"]["index"], 1);
    assert_eq!(response["focal_plane"]["z_offset_slide"], 2.5);
}

#[tokio::test]
async fn missing_sparse_position_is_explicitly_unavailable_and_never_reconstructed() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("missing.dcm");
    write_object(
        &path,
        InMemDicomObject::from_element_iter([
            DataElement::new(tags::SOP_CLASS_UID, VR::UI, WSI_STORAGE),
            DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, "2.25.9300"),
        ]),
    );
    let mut source = wsi_entry(path, 1);
    source.series_metadata.dimension_organization_type = Some("TILED_SPARSE".to_string());
    source.series_metadata.total_pixel_matrix_rows = Some(1_024);
    source.series_metadata.total_pixel_matrix_columns = Some(1_024);

    let server = TestServer::new(server::router(support::app_state(vec![source])));
    let response: Value = server.get("/api/file/0/frame/0/wsi-context").await.json();
    assert_eq!(response["positioning_status"], "unavailable");
    assert!(response["tile_rectangle"].is_null());
    assert_eq!(response["reconstruction_claimed"], false);
    assert!(response["warnings"][0]
        .as_str()
        .expect("warning")
        .contains("Per-Frame"));

    let out_of_range = server.get("/api/file/0/frame/1/wsi-context").await;
    out_of_range.assert_status_not_found();
    assert_eq!(out_of_range.json::<Value>()["code"], "frame_out_of_range");
}

fn wsi_entry(path: std::path::PathBuf, frame_count: u32) -> dcmview::types::FileEntry {
    let mut entry = support::file_entry(path, uids::EXPLICIT_VR_LITTLE_ENDIAN, frame_count);
    entry.sop_class_uid = WSI_STORAGE.to_string();
    entry.study_instance_uid = "2.25.study".to_string();
    entry
}

fn frame_group(row: i32, column: i32, optical_path: &str, z: f64) -> InMemDicomObject {
    let position = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::ROW_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(row),
        ),
        DataElement::new(
            tags::COLUMN_POSITION_IN_TOTAL_IMAGE_PIXEL_MATRIX,
            VR::SL,
            PrimitiveValue::from(column),
        ),
        DataElement::new(
            tags::Z_OFFSET_IN_SLIDE_COORDINATE_SYSTEM,
            VR::DS,
            z.to_string(),
        ),
    ]);
    let optical = InMemDicomObject::from_element_iter([DataElement::new(
        tags::OPTICAL_PATH_IDENTIFIER,
        VR::SH,
        optical_path,
    )]);
    InMemDicomObject::from_element_iter([
        sequence(tags::PLANE_POSITION_SLIDE_SEQUENCE, vec![position]),
        sequence(tags::OPTICAL_PATH_IDENTIFICATION_SEQUENCE, vec![optical]),
    ])
}

fn sequence(tag: dicom_core::Tag, items: Vec<InMemDicomObject>) -> DataElement<InMemDicomObject> {
    DataElement::new(tag, VR::SQ, DataSetSequence::from(items))
}

fn write_object(path: &Path, object: InMemDicomObject) {
    object
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(WSI_STORAGE)
                .media_storage_sop_instance_uid("2.25.9999"),
        )
        .expect("build WSI file meta")
        .write_to_file(path)
        .expect("write WSI fixture");
}
