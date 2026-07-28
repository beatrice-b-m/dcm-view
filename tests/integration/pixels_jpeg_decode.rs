use super::support;
use axum::http::{header, HeaderValue, StatusCode};
use axum_test::TestServer;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
use dcmview::server;
use image::ImageFormat;
use tempfile::tempdir;

#[tokio::test]
async fn decodes_jpeg_display_frame_to_png_and_sets_cache_hit_on_repeat() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("jpeg-frames.dcm");
    let frame0 = support::grayscale_jpeg_fragment_16x16(20);
    let frame1 = support::grayscale_jpeg_fragment_16x16(80);
    support::write_encapsulated_dicom(
        &path,
        "1.2.840.10008.1.2.4.50",
        vec![frame0.clone(), frame1.clone()],
    );

    let file = support::file_entry(path.clone(), "1.2.840.10008.1.2.4.50", 2);
    let cache = new_cache();

    let first = load_frame(
        file.clone(),
        cache.clone(),
        FrameRequest {
            frame: 1,
            window_center: None,
            window_width: None,
            window_mode: dcmview::types::WindowMode::Default,
        },
    )
    .await
    .expect("first decoded JPEG request");

    assert_eq!(first.content_type, "image/png");
    let first_image = image::load_from_memory_with_format(first.body.as_ref(), ImageFormat::Png)
        .expect("valid decoded JPEG PNG")
        .to_luma8();
    assert_eq!(first_image.width(), 16);
    assert_eq!(first_image.height(), 16);
    assert_ne!(
        first.body.as_ref(),
        frame1.as_slice(),
        "display endpoint must not return raw JPEG bytes"
    );
    assert!(!first.cache_hit);

    let second = load_frame(
        file,
        cache,
        FrameRequest {
            frame: 1,
            window_center: None,
            window_width: None,
            window_mode: dcmview::types::WindowMode::Default,
        },
    )
    .await
    .expect("second decoded JPEG request");

    assert_eq!(second.content_type, "image/png");
    assert_eq!(second.body, first.body);
    assert!(second.cache_hit);
}

#[tokio::test]
async fn jpeg_lossless_routes_through_server_decode() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("jpeg-lossless.dcm");
    // Arbitrary bytes that start with a JPEG SOI marker but are not valid JPEG Lossless data.
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

    // TS 4.70 must route through decode_frame_to_png instead, which fails on these invalid bytes.
    let is_raw_jpeg_response = response.status_code() == StatusCode::OK
        && response
            .maybe_header("content-type")
            .map(|v| v.to_str().unwrap_or("").starts_with("image/jpeg"))
            .unwrap_or(false)
        && response.as_bytes().as_ref() == frame.as_slice();
    assert!(
        !is_raw_jpeg_response,
        "TS 4.70 must not return raw JPEG bytes (status={})",
        response.status_code()
    );
}
