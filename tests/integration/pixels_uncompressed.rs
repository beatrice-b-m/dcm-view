use super::support;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
use dcmview::types::WindowPreset;
use image::ImageFormat;
use tempfile::tempdir;

#[tokio::test]
async fn decodes_uncompressed_png_and_tracks_window_cache_keys() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("uncompressed-le.dcm");
	support::write_uncompressed_u16_dicom(
		&path,
		"1.2.840.10008.1.2.1",
		2,
		2,
		vec![0, 1000, 2000, 3000, 500, 1500, 2500, 3500],
		Some("1500"),
		Some("3000"),
	);

	let mut entry = support::file_entry(path, "1.2.840.10008.1.2.1", 2);
	entry.rows = 2;
	entry.columns = 2;
	entry.default_window = Some(WindowPreset {
		center: 1500.0,
		width: 3000.0,
	});

	let files = vec![entry];
	let cache = new_cache();

	let first = load_frame(
		&files,
		cache.clone(),
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
	.expect("first uncompressed frame");
	assert_eq!(first.content_type, "image/png");
	assert!(!first.cache_hit);

	let first_image = image::load_from_memory_with_format(first.body.as_ref(), ImageFormat::Png)
		.expect("valid png")
		.to_luma8();
	assert_eq!(first_image.width(), 2);
	assert_eq!(first_image.height(), 2);

	let second = load_frame(
		&files,
		cache.clone(),
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
	.expect("second uncompressed frame");
	assert!(second.cache_hit);

	let overridden = load_frame(
		&files,
		cache,
		FrameRequest {
			file_index: 0,
			frame: 0,
			window_center: Some(800.0),
			window_width: Some(1000.0),
			window_mode: dcmview::types::WindowMode::Default,
			accept_header: Some("image/png".to_string()),
		},
	)
	.await
	.expect("window override frame");
	assert!(!overridden.cache_hit);
	assert_ne!(first.body.as_ref(), overridden.body.as_ref());
}

#[tokio::test]
async fn full_dynamic_mode_produces_distinct_output_and_has_independent_cache_key() {
	// Frame 0 pixels: [0, 1000, 2000, 3000]. True range: [0, 3000].
	//
	// default_window: center=1500, width=1000 → clips to [1000, 2000].
	//   0 → 0, 1000 → 0, 2000 → 255, 3000 → 255 (clipped at both ends)
	//
	// full_dynamic: min=0, max=3000 → window [0, 3000], no clipping.
	//   0 → 0, 1000 → 85, 2000 → 170, 3000 → 255
	//
	// These produce different images, proving the mode distinction works correctly.
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("uncompressed-window-mode.dcm");
	support::write_uncompressed_u16_dicom(
		&path,
		"1.2.840.10008.1.2.1",
		2,
		2,
		vec![0, 1000, 2000, 3000, 500, 1500, 2500, 3500],
		Some("1500"),
		Some("1000"), // narrow window: clips frame-0 values outside [1000, 2000]
	);

	let mut entry = support::file_entry(path, "1.2.840.10008.1.2.1", 2);
	entry.rows = 2;
	entry.columns = 2;
	entry.default_window = Some(WindowPreset {
		center: 1500.0,
		width: 1000.0, // narrow: clips [0, 3000] range at both ends
	});

	let files = vec![entry];
	let cache = new_cache();

	// Default mode: first request is a MISS.
	let default_first = load_frame(
		&files,
		cache.clone(),
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
	.expect("default mode first request");
	assert!(!default_first.cache_hit, "first default request must be a MISS");

	// Default mode: repeat is a HIT.
	let default_repeat = load_frame(
		&files,
		cache.clone(),
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
	.expect("default mode repeat");
	assert!(default_repeat.cache_hit, "repeat default request must be a HIT");

	// FullDynamic mode: same frame+wc+ww — must be a MISS because the cache key differs.
	let dynamic_first = load_frame(
		&files,
		cache.clone(),
		FrameRequest {
			file_index: 0,
			frame: 0,
			window_center: None,
			window_width: None,
			window_mode: dcmview::types::WindowMode::FullDynamic,
			accept_header: Some("image/png".to_string()),
		},
	)
	.await
	.expect("full_dynamic first request");
	assert!(!dynamic_first.cache_hit, "first full_dynamic request must be a MISS");

	// FullDynamic mode: repeat is a HIT.
	let dynamic_repeat = load_frame(
		&files,
		cache.clone(),
		FrameRequest {
			file_index: 0,
			frame: 0,
			window_center: None,
			window_width: None,
			window_mode: dcmview::types::WindowMode::FullDynamic,
			accept_header: Some("image/png".to_string()),
		},
	)
	.await
	.expect("full_dynamic repeat");
	assert!(dynamic_repeat.cache_hit, "repeat full_dynamic request must be a HIT");

	// The two mode outputs must produce different pixel data because the window ranges differ.
	assert_ne!(
		default_first.body.as_ref(),
		dynamic_first.body.as_ref(),
		"full_dynamic output must differ from default-windowed output"
	);
}

#[tokio::test]
async fn applies_big_endian_byte_order_for_uncompressed_pixels() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("uncompressed-be.dcm");
	support::write_uncompressed_u16_dicom(
		&path,
		"1.2.840.10008.1.2.2",
		1,
		2,
		vec![256, 1],
		Some("128"),
		Some("256"),
	);

	let mut entry = support::file_entry(path, "1.2.840.10008.1.2.2", 1);
	entry.rows = 1;
	entry.columns = 2;
	entry.default_window = Some(WindowPreset {
		center: 128.0,
		width: 256.0,
	});

	let response = load_frame(
		&[entry],
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
	.expect("big-endian frame decode");

	let image = image::load_from_memory_with_format(response.body.as_ref(), ImageFormat::Png)
		.expect("valid png")
		.to_luma8();
	let first = image.get_pixel(0, 0).0[0];
	let second = image.get_pixel(1, 0).0[0];
	assert!(first > second, "expected first pixel to remain brighter after BE decode");
}
