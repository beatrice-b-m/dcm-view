use super::support;
use axum_test::TestServer;
use dcmview::annotations::EmbedRoiAnnotations;
use dcmview::server;
use serde_json::Value;
use std::collections::HashMap;
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
