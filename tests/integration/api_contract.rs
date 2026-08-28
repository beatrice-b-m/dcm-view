use super::support;
use axum::http::{header, HeaderValue, StatusCode};
use axum_test::{TestResponse, TestServer};
use bytes::Bytes;
use dcmview::annotations::{AnnotationStore, EmbedRoiAnnotations};
use dcmview::api::contracts::{
    ApiEndpointContract, ApiMethod, ApiResponseHeadersKind, API_ENDPOINTS, API_PREFIX,
    CACHE_HEADER, CACHE_HIT, CACHE_MISS, EXPORT_CONTENT_DISPOSITION_HEADER,
    EXPORT_CONTENT_DISPOSITION_VALUE, RAW_FRAME_HEADERS,
};
use dcmview::server;
use dcmview::types::WindowPreset;
use serde_json::Value;
use std::collections::HashMap;
use tempfile::tempdir;

#[tokio::test]
async fn json_endpoints_match_frontend_contract_shapes() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("contract.dcm");
    support::write_uncompressed_u16_dicom(
        &path,
        "1.2.840.10008.1.2.1",
        2,
        2,
        vec![0, 1000, 2000, 3000],
        Some("1500"),
        Some("3000"),
    );

    let mut entry = support::file_entry(path, "1.2.840.10008.1.2.1", 1);
    entry.rows = 2;
    entry.columns = 2;
    entry.default_window = Some(WindowPreset {
        center: 1500.0,
        width: 3000.0,
    });

    let state = support::app_state_with_annotations(
        vec![entry],
        AnnotationStore::new(HashMap::from([(
            0,
            EmbedRoiAnnotations {
                num_roi: 1,
                roi_coords: vec![[1, 2, 3, 4]],
                roi_frames: vec![vec![0]],
            },
        )])),
    );

    let test_server = TestServer::new(server::router(state));

    let files: Value = test_server.get("/api/files").await.json();
    assert_object_keys(
        &files,
        &[
            "files",
            "filtered",
            "scan_complete",
            "scanned",
            "server_start_ms",
            "skipped",
            "tunnel_host",
            "tunnelled",
        ],
    );
    let file = &files["files"].as_array().expect("files array")[0];
    assert_object_keys(
        file,
        &[
            "columns",
            "default_window",
            "frame_count",
            "has_pixels",
            "index",
            "instance_number",
            "label",
            "modality",
            "path",
            "patient_id",
            "patient_name",
            "rows",
            "series_description",
            "series_instance_uid",
            "series_number",
            "sop_instance_uid",
            "study_date",
            "study_description",
            "study_instance_uid",
            "transfer_syntax_uid",
        ],
    );
    assert_object_keys(&file["default_window"], &["center", "width"]);

    let info: Value = test_server.get("/api/file/0/info").await.json();
    assert_object_keys(
        &info,
        &[
            "columns",
            "default_window",
            "frame_count",
            "has_pixels",
            "rows",
            "transfer_syntax_uid",
        ],
    );
    assert_object_keys(&info["default_window"], &["center", "width"]);

    let tags: Value = test_server.get("/api/file/0/tags").await.json();
    let tag_rows = tags.as_array().expect("tag response array");
    let rows_tag = tag_rows
        .iter()
        .find(|row| row["keyword"] == "Rows")
        .expect("Rows tag");
    assert_tag_node_shape(rows_tag);
    assert_object_keys(&rows_tag["value"], &["type", "value"]);
    assert_eq!(rows_tag["value"]["type"], "number");

    let pixel_data_tag = tag_rows
        .iter()
        .find(|row| row["tag"] == "(7FE0,0010)")
        .expect("PixelData tag");
    assert_tag_node_shape(pixel_data_tag);
    assert_object_keys(&pixel_data_tag["value"], &["length", "type"]);
    assert_eq!(pixel_data_tag["value"]["type"], "binary");

    let annotations: Value = test_server.get("/api/file/0/annotations").await.json();
    assert_object_keys(&annotations, &["num_roi", "roi_coords", "roi_frames"]);
    assert_eq!(
        annotations["roi_coords"][0],
        serde_json::json!([1, 2, 3, 4])
    );
    assert_eq!(annotations["roi_frames"][0], serde_json::json!([0]));

    let missing = test_server.get("/api/file/99/info").await;
    missing.assert_status_not_found();
    let error: Value = missing.json();
    assert_object_keys(&error, &["code", "error"]);
    assert_eq!(error["code"], "not_found");
    assert_eq!(error["error"], "file index out of range");
}

#[tokio::test]
async fn every_declared_endpoint_matches_its_runtime_contract() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("endpoint-registry.dcm");
    support::write_uncompressed_u16_dicom(
        &path,
        "1.2.840.10008.1.2.1",
        2,
        2,
        vec![0, 1000, 2000, 3000],
        Some("1500"),
        Some("3000"),
    );
    let mut entry = support::file_entry(path, "1.2.840.10008.1.2.1", 1);
    entry.rows = 2;
    entry.columns = 2;
    entry.default_window = Some(WindowPreset {
        center: 1500.0,
        width: 3000.0,
    });
    let test_server = TestServer::new(server::router(support::app_state(vec![entry])));
    let annotation_body = EmbedRoiAnnotations::empty();

    for endpoint in API_ENDPOINTS {
        let path = format!("{API_PREFIX}{}", endpoint.path)
            .replace("{index}", "0")
            .replace("{frame}", "0");
        let request = match endpoint.method {
            ApiMethod::Get => test_server.get(&path),
            ApiMethod::Put => test_server.put(&path).json(&annotation_body),
        };
        let response = request.await;

        assert_eq!(
            response.status_code().as_u16(),
            endpoint.success_status,
            "{} status contract",
            endpoint.id
        );
        assert_eq!(
            response
                .header(header::CONTENT_TYPE)
                .to_str()
                .expect("content type"),
            endpoint.response_media_type,
            "{} media-type contract",
            endpoint.id
        );
        assert_declared_response_headers(endpoint, &response);
    }
}

#[tokio::test]
async fn raw_frame_endpoint_exposes_frontend_metadata_header_contract() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("raw-contract.dcm");
    support::write_uncompressed_u16_dicom(
        &path,
        "1.2.840.10008.1.2.1",
        2,
        2,
        vec![0, 1000, 2000, 3000],
        Some("1500"),
        Some("3000"),
    );

    let mut entry = support::file_entry(path, "1.2.840.10008.1.2.1", 1);
    entry.rows = 2;
    entry.columns = 2;
    entry.default_window = Some(WindowPreset {
        center: 1500.0,
        width: 3000.0,
    });
    let has_default_window = entry.default_window.is_some();

    let test_server = TestServer::new(server::router(support::app_state(vec![entry])));
    let response = test_server.get("/api/file/0/frame/0/raw").await;
    response.assert_status_ok();

    assert_eq!(
        response
            .header(header::CONTENT_TYPE)
            .to_str()
            .expect("content-type"),
        "application/octet-stream"
    );
    assert!(response.maybe_header(CACHE_HEADER).is_some());
    for header_contract in RAW_FRAME_HEADERS {
        let present = response.maybe_header(header_contract.name).is_some();
        if matches!(header_contract.field, "defaultWc" | "defaultWw") {
            assert_eq!(
                present, has_default_window,
                "optional raw header {} presence",
                header_contract.name
            );
        } else {
            assert!(present, "raw response missing {}", header_contract.name);
        }
    }
}

#[tokio::test]
async fn api_boundary_rejections_use_the_json_error_envelope() {
    let test_server = TestServer::new(server::router(support::app_state(Vec::new())));

    let cases = vec![
        (
            "malformed path",
            test_server.get("/api/file/not-a-number/info").await,
            StatusCode::BAD_REQUEST,
        ),
        (
            "malformed query",
            test_server.get("/api/file/0/frame/0?wc=not-a-number").await,
            StatusCode::BAD_REQUEST,
        ),
        (
            "malformed JSON",
            test_server
                .put("/api/file/0/annotations")
                .bytes(Bytes::from_static(b"{"))
                .add_header(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                )
                .await,
            StatusCode::BAD_REQUEST,
        ),
        (
            "invalid JSON shape",
            test_server
                .put("/api/file/0/annotations")
                .json(&serde_json::json!({}))
                .await,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "missing JSON content type",
            test_server
                .put("/api/file/0/annotations")
                .bytes(Bytes::from_static(b"{}"))
                .await,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "unknown API route",
            test_server.get("/api/not-a-route").await,
            StatusCode::NOT_FOUND,
        ),
        (
            "wrong API method",
            test_server.post("/api/files").await,
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ];

    for (name, response, expected_status) in cases {
        assert_json_error(name, &response, expected_status);
    }
}

#[tokio::test]
async fn display_frame_rejects_invalid_window_queries_as_json() {
    let entry = support::file_entry("window-validation.dcm".into(), "1.2.840.10008.1.2.1", 1);
    let test_server = TestServer::new(server::router(support::app_state(vec![entry])));

    for query in [
        "wc=10",
        "ww=20",
        "wc=NaN&ww=20",
        "wc=10&ww=NaN",
        "wc=10&ww=0",
        "wc=10&ww=-1",
    ] {
        let response = test_server
            .get(&format!("/api/file/0/frame/0?{query}"))
            .await;
        assert_json_error(query, &response, StatusCode::BAD_REQUEST);
    }
}

fn assert_json_error(name: &str, response: &TestResponse, expected_status: StatusCode) {
    assert_eq!(response.status_code(), expected_status, "{name}");
    assert!(
        response
            .header(header::CONTENT_TYPE)
            .to_str()
            .expect("content type")
            .starts_with("application/json"),
        "{name} must return JSON"
    );
    let payload: Value = response.json();
    assert_object_keys(&payload, &["code", "error"]);
    assert!(
        payload["error"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "{name} must include a non-empty error message"
    );
}

fn assert_declared_response_headers(endpoint: &ApiEndpointContract, response: &TestResponse) {
    match endpoint.response_headers {
        ApiResponseHeadersKind::None => {
            assert_no_cache_header(endpoint, response);
            assert_no_raw_frame_headers(endpoint, response);
            assert_no_export_header(endpoint, response);
        }
        ApiResponseHeadersKind::Cache => {
            assert_cache_header(endpoint, response);
            assert_no_raw_frame_headers(endpoint, response);
            assert_no_export_header(endpoint, response);
        }
        ApiResponseHeadersKind::RawFrame => {
            assert_cache_header(endpoint, response);
            for raw_header in RAW_FRAME_HEADERS {
                assert!(
                    response.maybe_header(raw_header.name).is_some(),
                    "{} is missing raw-frame header {}",
                    endpoint.id,
                    raw_header.name
                );
            }
            assert_no_export_header(endpoint, response);
        }
        ApiResponseHeadersKind::Export => {
            assert_no_cache_header(endpoint, response);
            assert_no_raw_frame_headers(endpoint, response);
            assert_eq!(
                response
                    .header(EXPORT_CONTENT_DISPOSITION_HEADER)
                    .to_str()
                    .expect("content-disposition"),
                EXPORT_CONTENT_DISPOSITION_VALUE,
                "{} content-disposition contract",
                endpoint.id
            );
        }
    }
}

fn assert_cache_header(endpoint: &ApiEndpointContract, response: &TestResponse) {
    let header = response.header(CACHE_HEADER);
    let value = header.to_str().expect("cache header");
    assert!(
        matches!(value, CACHE_HIT | CACHE_MISS),
        "{} returned invalid {CACHE_HEADER} value {value:?}",
        endpoint.id
    );
}

fn assert_no_cache_header(endpoint: &ApiEndpointContract, response: &TestResponse) {
    assert!(
        response.maybe_header(CACHE_HEADER).is_none(),
        "{} unexpectedly returned {CACHE_HEADER}",
        endpoint.id
    );
}

fn assert_no_raw_frame_headers(endpoint: &ApiEndpointContract, response: &TestResponse) {
    for raw_header in RAW_FRAME_HEADERS {
        assert!(
            response.maybe_header(raw_header.name).is_none(),
            "{} unexpectedly returned raw-frame header {}",
            endpoint.id,
            raw_header.name
        );
    }
}

fn assert_no_export_header(endpoint: &ApiEndpointContract, response: &TestResponse) {
    assert!(
        response
            .maybe_header(EXPORT_CONTENT_DISPOSITION_HEADER)
            .is_none(),
        "{} unexpectedly returned {EXPORT_CONTENT_DISPOSITION_HEADER}",
        endpoint.id
    );
}

fn assert_tag_node_shape(value: &Value) {
    assert_object_keys(value, &["keyword", "tag", "value", "vr"]);
    assert!(value["tag"].as_str().expect("tag string").starts_with('('));
    assert!(value["vr"].is_string());
    assert!(value["keyword"].is_string());
    assert!(value["value"]["type"].is_string());
}

fn assert_object_keys(value: &Value, expected: &[&str]) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("expected object, got {value:?}"));
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    actual.sort_unstable();

    let mut expected = expected.to_vec();
    expected.sort_unstable();

    assert_eq!(actual, expected);
}
