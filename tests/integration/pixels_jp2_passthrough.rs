use super::support;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
use tempfile::tempdir;

#[tokio::test]
async fn returns_raw_jp2_when_accept_header_supports_it() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("jp2-frames.dcm");
	let frame = vec![0x00, 0x00, 0x00, 0x0c, b'j', b'P', b' ', b' ', 0x0d, 0x0a, 0x87, 0x0a];
	support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.91", vec![frame.clone()]);

	let files = vec![support::file_entry(path, "1.2.840.10008.1.2.4.91", 1)];
	let response = load_frame(
		&files,
		new_cache(),
		FrameRequest {
			file_index: 0,
			frame: 0,
			window_center: None,
			window_width: None,
			window_mode: dcmview::types::WindowMode::Default,
			accept_header: Some("image/jp2,image/*;q=0.8".to_string()),
		},
	)
	.await
	.expect("jp2 passthrough request");

	assert_eq!(response.content_type, "image/jp2");
	assert_eq!(response.body.to_vec(), frame);
	assert!(!response.cache_hit);
}

#[tokio::test]
async fn falls_back_to_decode_when_accept_lacks_jp2_and_surfaces_error_on_invalid_data() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("jp2-fallback.dcm");
	support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.91", vec![vec![1, 2, 3, 4]]);

	let files = vec![support::file_entry(path, "1.2.840.10008.1.2.4.91", 1)];
	let error = load_frame(
		&files,
		new_cache(),
		FrameRequest {
			file_index: 0,
			frame: 0,
			window_center: None,
			window_width: None,
			window_mode: dcmview::types::WindowMode::Default,
			accept_header: Some("image/png".to_string()),
		},
	)
	.await
	.expect_err("invalid JP2 payload should fail fallback decoding");

	assert!(
		error.to_string().contains("failed to decode JP2 fragment"),
		"fallback path should surface JP2 decode failure: {error}"
	);
}
