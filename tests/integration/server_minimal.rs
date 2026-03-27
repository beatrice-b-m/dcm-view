use super::support;
use axum::http::{header, HeaderValue};
use axum_test::TestServer;
use dcmview::server;
use serde_json::Value;
use tempfile::tempdir;

#[tokio::test]
async fn exposes_files_info_and_frame_endpoints_with_cache_headers() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("server-jpeg.dcm");
	let frame = vec![0xff, 0xd8, 0xff, 0xdb, 0x00, 0x00];
	support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.50", vec![frame.clone()]);

	let app = server::router(support::app_state(vec![support::file_entry(
		path,
		"1.2.840.10008.1.2.4.50",
		1,
	)]));
	let test_server = TestServer::new(app).expect("test server");

	let files_response = test_server.get("/api/files").await;
	files_response.assert_status_ok();
	let files_json: Value = files_response.json();
	assert_eq!(files_json["files"].as_array().expect("files array").len(), 1);
	assert_eq!(files_json["tunnelled"], false);

	let info_response = test_server.get("/api/file/0/info").await;
	info_response.assert_status_ok();
	let info_json: Value = info_response.json();
	assert_eq!(info_json["frame_count"], 1);
	assert_eq!(info_json["transfer_syntax"], "1.2.840.10008.1.2.4.50");

	let first_frame = test_server
		.get("/api/file/0/frame/0")
		.add_header(header::ACCEPT, HeaderValue::from_static("image/jpeg"))
		.await;
	first_frame.assert_status_ok();
	assert_eq!(first_frame.header("X-Cache").to_str().expect("cache header"), "MISS");
	assert_eq!(first_frame.as_bytes().as_ref(), frame.as_slice());

	let second_frame = test_server
		.get("/api/file/0/frame/0")
		.add_header(header::ACCEPT, HeaderValue::from_static("image/jpeg"))
		.await;
	second_frame.assert_status_ok();
	assert_eq!(second_frame.header("X-Cache").to_str().expect("cache header"), "HIT");
}

#[tokio::test]
async fn returns_not_found_for_out_of_range_frame() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("server-jpeg.dcm");
	support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.50", vec![vec![1, 2, 3, 4]]);

	let app = server::router(support::app_state(vec![support::file_entry(
		path,
		"1.2.840.10008.1.2.4.50",
		1,
	)]));
	let test_server = TestServer::new(app).expect("test server");

	let response = test_server.get("/api/file/0/frame/3").await;
	response.assert_status_not_found();
}
