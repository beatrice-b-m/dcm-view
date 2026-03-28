use super::support;
use axum::http::{header, HeaderValue, StatusCode};
use axum_test::TestServer;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
use dcmview::server;
use tempfile::tempdir;

#[tokio::test]
async fn returns_raw_jpeg_fragment_and_sets_cache_hit_on_repeat() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("jpeg-frames.dcm");
	let frame0 = vec![0xff, 0xd8, 0xff, 0xdb, 0x00, 0x00];
	let frame1 = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x01, 0x02, 0x03];
	support::write_encapsulated_dicom(
		&path,
		"1.2.840.10008.1.2.4.50",
		vec![frame0.clone(), frame1.clone()],
	);

	let files = vec![support::file_entry(path.clone(), "1.2.840.10008.1.2.4.50", 2)];
	let cache = new_cache();

	let first = load_frame(
		&files,
		cache.clone(),
		FrameRequest {
			file_index: 0,
			frame: 1,
			window_center: None,
			window_width: None,
			accept_header: Some("image/jpeg".to_string()),
		},
	)
	.await
	.expect("first passthrough request");

	assert_eq!(first.content_type, "image/jpeg");
	assert_eq!(first.body.to_vec(), frame1);
	assert!(!first.cache_hit);

	let second = load_frame(
		&files,
		cache,
		FrameRequest {
			file_index: 0,
			frame: 1,
			window_center: None,
			window_width: None,
			accept_header: Some("image/jpeg".to_string()),
		},
	)
	.await
	.expect("second passthrough request");

	assert_eq!(second.content_type, "image/jpeg");
	assert_eq!(second.body.to_vec(), vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x01, 0x02, 0x03]);
	assert!(second.cache_hit);
}

#[tokio::test]
async fn decodes_jpeg_lossless_to_png_instead_of_passthrough() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("jpeg-lossless.dcm");
	// Arbitrary bytes that start with a JPEG SOI marker but are not valid JPEG Lossless data.
	// The passthrough path (TS 4.50) would return these verbatim as 200 + image/jpeg.
	// The decode path (TS 4.70) will attempt decode_frame_to_png and fail on invalid data.
	let frame = vec![0xFF_u8, 0xD8, 0xFF, 0xDB, 0x00, 0x01];
	support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.70", vec![frame.clone()]);

	let app = server::router(support::app_state(vec![support::file_entry(
		path,
		"1.2.840.10008.1.2.4.70",
		1,
	)]));
	let test_server = TestServer::new(app);

	let response = test_server
		.get("/api/file/0/frame/0")
		.add_header(header::ACCEPT, HeaderValue::from_static("image/jpeg"))
		.await;

	// Passthrough would return 200 + image/jpeg content-type + the exact raw fragment bytes.
	// TS 4.70 must route through decode_frame_to_png instead, which fails on these invalid bytes.
	let is_passthrough = response.status_code() == StatusCode::OK
		&& response
			.maybe_header("content-type")
			.map(|v| v.to_str().unwrap_or("").starts_with("image/jpeg"))
			.unwrap_or(false)
		&& response.as_bytes().as_ref() == frame.as_slice();
	assert!(
		!is_passthrough,
		"TS 4.70 must not be treated as browser-passthrough JPEG (status={})",
		response.status_code()
	);
}
