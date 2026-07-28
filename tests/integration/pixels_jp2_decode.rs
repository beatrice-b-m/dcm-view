use super::support;
use dcmview::pixels::{load_frame, new_cache, FrameRequest};
use tempfile::tempdir;

#[tokio::test]
async fn invalid_jp2_payload_surfaces_server_side_decode_error() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("jp2-frames.dcm");
    let frame = vec![
        0x00, 0x00, 0x00, 0x0c, b'j', b'P', b' ', b' ', 0x0d, 0x0a, 0x87, 0x0a,
    ];
    support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.91", vec![frame.clone()]);

    let file = support::file_entry(path, "1.2.840.10008.1.2.4.91", 1);
    let error = load_frame(
        file,
        new_cache(),
        FrameRequest {
            frame: 0,
            window_center: None,
            window_width: None,
            window_mode: dcmview::types::WindowMode::Default,
        },
    )
    .await
    .expect_err("invalid JP2 payload should fail server-side decoding");

    assert!(
        error.to_string().contains("failed to decode JP2 fragment"),
        "JP2 display path should decode instead of returning raw bytes: {error}"
    );
}

#[tokio::test]
async fn invalid_jp2_codestream_surfaces_decode_context() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("jp2-fallback.dcm");
    support::write_encapsulated_dicom(&path, "1.2.840.10008.1.2.4.91", vec![vec![1, 2, 3, 4]]);

    let file = support::file_entry(path, "1.2.840.10008.1.2.4.91", 1);
    let error = load_frame(
        file,
        new_cache(),
        FrameRequest {
            frame: 0,
            window_center: None,
            window_width: None,
            window_mode: dcmview::types::WindowMode::Default,
        },
    )
    .await
    .expect_err("invalid JP2 payload should fail fallback decoding");

    assert!(
        error.to_string().contains("failed to decode JP2 fragment"),
        "fallback path should surface JP2 decode failure: {error}"
    );
}
