use super::support;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
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
