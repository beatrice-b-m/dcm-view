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
	let test_server = TestServer::new(app);

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
	let test_server = TestServer::new(app);

	let response = test_server.get("/api/file/0/frame/3").await;
	response.assert_status_not_found();
}


#[tokio::test]
async fn serves_embedded_frontend_shell_at_root() {
	let app = server::router(support::app_state(Vec::new()));
	let test_server = TestServer::new(app);

	let response = test_server.get("/").await;
	response.assert_status_ok();
	assert!(
		response
			.header(header::CONTENT_TYPE)
			.to_str()
			.expect("content-type header")
			.starts_with("text/html"),
		"root endpoint should return embedded index.html"
	);
}

#[tokio::test]
async fn serves_js_and_css_assets_with_correct_mime_types() {
	let app = server::router(support::app_state(Vec::new()));
	let test_server = TestServer::new(app);

	// Discover actual asset filenames from the index.html body
	let index_body = test_server.get("/").await.text();

	// Extract JS asset path from: src="/assets/index-XXXX.js"
	let js_path = index_body
		.split("src=\"/assets/")
		.nth(1)
		.and_then(|s| s.split('"').next())
		.expect("JS asset referenced in index.html");

	// Extract CSS asset path from: href="/assets/index-XXXX.css"
	let css_path = index_body
		.split("href=\"/assets/")
		.nth(1)
		.and_then(|s| s.split('"').next())
		.expect("CSS asset referenced in index.html");

	let js_response = test_server.get(&format!("/assets/{js_path}")).await;
	js_response.assert_status_ok();
	let js_ct_header = js_response.header(header::CONTENT_TYPE);
	let js_ct = js_ct_header.to_str().expect("js content-type");
	assert!(
		js_ct.starts_with("text/javascript"),
		"JS asset should be text/javascript, got: {js_ct}"
	);
	assert!(!js_response.as_bytes().is_empty(), "JS body should be non-empty");

	let css_response = test_server.get(&format!("/assets/{css_path}")).await;
	css_response.assert_status_ok();
	let css_ct_header = css_response.header(header::CONTENT_TYPE);
	let css_ct = css_ct_header.to_str().expect("css content-type");
	assert!(
		css_ct.starts_with("text/css"),
		"CSS asset should be text/css, got: {css_ct}"
	);
	assert!(!css_response.as_bytes().is_empty(), "CSS body should be non-empty");
}