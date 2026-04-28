use super::support;
use axum_test::TestServer;
use dcmview::annotations::{self, EmbedRoiAnnotations};
use dcmview::server;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn returns_annotations_for_matching_file_index() {
    let dir = tempdir().expect("temp dir");
    let entry = support::file_entry(
        dir.path().join("annotated.dcm"),
        "1.2.840.10008.1.2.4.50",
        10,
    );
    let mut state = support::app_state(vec![entry]);
    state.annotations = Arc::new(HashMap::from([(
        0,
        EmbedRoiAnnotations {
            num_roi: 2,
            roi_coords: vec![[11, 22, 33, 44], [55, 66, 77, 88]],
            roi_frames: vec![vec![0, 1, 2], vec![3]],
        },
    )]));

    let app = server::router(state);
    let test_server = TestServer::new(app);

    let response = test_server.get("/api/file/0/annotations").await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["num_roi"], 2);
    assert_eq!(body["roi_coords"][0], serde_json::json!([11, 22, 33, 44]));
    assert_eq!(body["roi_coords"][1], serde_json::json!([55, 66, 77, 88]));
    assert_eq!(body["roi_frames"][0], serde_json::json!([0, 1, 2]));
    assert_eq!(body["roi_frames"][1], serde_json::json!([3]));
}

#[tokio::test]
async fn returns_empty_annotations_payload_when_file_has_no_match() {
    let dir = tempdir().expect("temp dir");
    let entry = support::file_entry(
        dir.path().join("unannotated.dcm"),
        "1.2.840.10008.1.2.4.50",
        1,
    );
    let app = server::router(support::app_state(vec![entry]));
    let test_server = TestServer::new(app);

    let response = test_server.get("/api/file/0/annotations").await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["num_roi"], 0);
    assert_eq!(body["roi_coords"], serde_json::json!([]));
    assert_eq!(body["roi_frames"], serde_json::json!([]));
}

#[tokio::test]
async fn returns_not_found_for_out_of_range_file_index() {
    let app = server::router(support::app_state(Vec::new()));
    let test_server = TestServer::new(app);

    let response = test_server.get("/api/file/99/annotations").await;
    response.assert_status_not_found();
    let body: Value = response.json();
    assert_eq!(body["error"], "file index out of range");
}

/// Exercises the full pipeline: CSV parsing → path matching → HTTP response.
/// The earlier tests manually insert annotations into AppState, bypassing
/// `load_annotations_for_files`. This test confirms the two halves compose
/// correctly: a CSV row whose path matches a loaded FileEntry must produce a
/// non-empty annotation response for that file's index.
#[tokio::test]
async fn serves_annotations_loaded_from_csv_for_matching_file() {
    let dir = tempdir().expect("temp dir");
    let dcm_path = dir.path().join("annotated.dcm");
    let csv_path = dir.path().join("annotations.csv");

    support::write_encapsulated_dicom(&dcm_path, "1.2.840.10008.1.2.4.50", vec![vec![0u8; 16 * 16]]);

    fs::write(
        &csv_path,
        format!(
            "anon_dicom_path,ROI_coords,ROI_frames\n{},\"[[10,20,30,40]]\",\"[[0]]\"\n",
            dcm_path.display()
        ),
    )
    .expect("write annotations CSV");

    let entry = support::file_entry(dcm_path, "1.2.840.10008.1.2.4.50", 1);
    let annotation_map = annotations::load_annotations_for_files(&csv_path, &[entry.clone()])
        .expect("load_annotations_for_files should succeed");

    assert!(
        !annotation_map.is_empty(),
        "annotation_map is empty — CSV path did not match FileEntry path; path normalization may be broken"
    );

    let mut state = support::app_state(vec![entry]);
    state.annotations = Arc::new(annotation_map);

    let app = server::router(state);
    let test_server = TestServer::new(app);

    let response = test_server.get("/api/file/0/annotations").await;
    response.assert_status_ok();

    let body: Value = response.json();
    assert_eq!(body["num_roi"], 1, "expected 1 ROI from CSV");
    assert_eq!(body["roi_coords"][0], serde_json::json!([10, 20, 30, 40]));
    assert_eq!(body["roi_frames"][0], serde_json::json!([0]));
}
