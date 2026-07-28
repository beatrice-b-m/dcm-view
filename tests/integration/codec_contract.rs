use super::support;
use axum::http::StatusCode;
use axum_test::{TestResponse, TestServer};
use dcmview::loader::{self, DiscoverOptions};
use dcmview::pixels;
use dcmview::server;
use dcmview::types::TransferSyntaxClass;
use image::ImageFormat;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn header(response: &TestResponse, name: &str) -> String {
    response
        .header(name)
        .to_str()
        .expect("response header should be valid UTF-8")
        .to_string()
}

async fn assert_compressed_fixture_contract(
    name: &str,
    transfer_syntax_uid: &str,
    rows: u32,
    columns: u32,
    bits_allocated: u32,
    default_window: (f64, f64),
    expected_raw: Vec<u8>,
) {
    let report = loader::discover(
        &[fixture_path(name)],
        DiscoverOptions {
            recursive: false,
            filters: Vec::new(),
        },
    )
    .await
    .expect("discover compressed golden fixture");
    assert_eq!(report.files.len(), 1);

    let file = &report.files[0];
    assert!(file.has_pixels);
    assert_eq!(file.frame_count, 1);
    assert_eq!(file.rows, rows);
    assert_eq!(file.columns, columns);
    assert_eq!(file.bits_allocated, bits_allocated);
    assert_eq!(file.samples_per_pixel, 1);
    assert_eq!(file.photometric_interpretation, "MONOCHROME2");
    assert_eq!(file.transfer_syntax_uid, transfer_syntax_uid);

    let test_server = TestServer::new(server::router(support::app_state(report.files)));

    let first_display = test_server.get("/api/file/0/frame/0").await;
    first_display.assert_status_ok();
    assert_eq!(header(&first_display, "content-type"), "image/png");
    assert_eq!(header(&first_display, "x-cache"), "MISS");
    let display_image =
        image::load_from_memory_with_format(first_display.as_bytes().as_ref(), ImageFormat::Png)
            .expect("display endpoint should return a valid PNG");
    assert_eq!(display_image.height(), rows);
    assert_eq!(display_image.width(), columns);

    let second_display = test_server.get("/api/file/0/frame/0").await;
    second_display.assert_status_ok();
    assert_eq!(header(&second_display, "x-cache"), "HIT");
    assert_eq!(first_display.as_bytes(), second_display.as_bytes());

    let first_raw = test_server.get("/api/file/0/frame/0/raw").await;
    first_raw.assert_status_ok();
    assert_eq!(
        header(&first_raw, "content-type"),
        "application/octet-stream"
    );
    assert_eq!(header(&first_raw, "x-cache"), "MISS");
    assert_eq!(header(&first_raw, "x-frame-rows"), rows.to_string());
    assert_eq!(header(&first_raw, "x-frame-columns"), columns.to_string());
    assert_eq!(
        header(&first_raw, "x-frame-bits-allocated"),
        bits_allocated.to_string()
    );
    assert_eq!(header(&first_raw, "x-frame-pixel-representation"), "0");
    assert_eq!(header(&first_raw, "x-frame-samples-per-pixel"), "1");
    assert_eq!(
        header(&first_raw, "x-frame-photometric-interpretation"),
        "MONOCHROME2"
    );
    assert_eq!(header(&first_raw, "x-frame-rescale-slope"), "1");
    assert_eq!(header(&first_raw, "x-frame-rescale-intercept"), "0");
    assert_eq!(
        header(&first_raw, "x-frame-default-wc"),
        default_window.0.to_string()
    );
    assert_eq!(
        header(&first_raw, "x-frame-default-ww"),
        default_window.1.to_string()
    );
    assert_eq!(first_raw.as_bytes().as_ref(), expected_raw);

    let second_raw = test_server.get("/api/file/0/frame/0/raw").await;
    second_raw.assert_status_ok();
    assert_eq!(header(&second_raw, "x-cache"), "HIT");
    assert_eq!(first_raw.as_bytes(), second_raw.as_bytes());
}

#[tokio::test]
async fn jpeg_lossless_fixture_satisfies_display_and_raw_contracts() {
    let samples = [
        0_u16, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000, 1100, 1200, 1300, 1400, 1500,
    ];
    let expected_raw = samples
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();

    assert_compressed_fixture_contract(
        "golden-jpeg-lossless-u16-single-frame.dcm",
        "1.2.840.10008.1.2.4.70",
        4,
        4,
        16,
        (750.0, 1500.0),
        expected_raw,
    )
    .await;
}

#[tokio::test]
async fn jpeg2000_lossless_fixture_satisfies_display_and_raw_contracts() {
    let mut expected_raw = Vec::with_capacity(16 * 16);
    for row in 0_u8..16 {
        expected_raw.extend_from_slice(&[row * 17; 16]);
    }

    assert_compressed_fixture_contract(
        "golden-jpeg2000-lossless-u8-single-frame.dcm",
        "1.2.840.10008.1.2.4.90",
        16,
        16,
        8,
        (127.5, 255.0),
        expected_raw,
    )
    .await;
}

#[test]
fn transfer_syntax_classification_table_covers_every_supported_status() {
    let cases = [
        ("1.2.840.10008.1.2.4.50", TransferSyntaxClass::Jpeg),
        ("1.2.840.10008.1.2.4.51", TransferSyntaxClass::Jpeg),
        ("1.2.840.10008.1.2.4.57", TransferSyntaxClass::JpegLossless),
        ("1.2.840.10008.1.2.4.70", TransferSyntaxClass::JpegLossless),
        ("1.2.840.10008.1.2.4.90", TransferSyntaxClass::Jpeg2000),
        ("1.2.840.10008.1.2.4.91", TransferSyntaxClass::Jpeg2000),
        ("1.2.840.10008.1.2", TransferSyntaxClass::Uncompressed),
        ("1.2.840.10008.1.2.1", TransferSyntaxClass::Uncompressed),
        ("1.2.840.10008.1.2.2", TransferSyntaxClass::Uncompressed),
        ("1.2.840.10008.1.2.4.80", TransferSyntaxClass::JpegLs),
        ("1.2.840.10008.1.2.4.81", TransferSyntaxClass::JpegLs),
        ("1.2.840.10008.1.2.5", TransferSyntaxClass::Rle),
        ("9.9.9", TransferSyntaxClass::Unsupported),
    ];

    for (uid, expected) in cases {
        assert_eq!(
            pixels::classify_transfer_syntax(uid),
            expected,
            "classification mismatch for transfer syntax {uid}"
        );
    }
}

#[tokio::test]
async fn unsupported_syntax_classes_return_422_from_both_frame_endpoints() {
    let dir = tempdir().expect("temporary fixture directory");
    let cases = [
        ("jpeg-ls-lossless", "1.2.840.10008.1.2.4.80"),
        ("jpeg-ls-near-lossless", "1.2.840.10008.1.2.4.81"),
        ("rle", "1.2.840.10008.1.2.5"),
        ("unknown", "9.9.9"),
    ];

    for (label, transfer_syntax_uid) in cases {
        let path = dir.path().join(format!("{label}.dcm"));
        // Unknown syntaxes are rejected from registry metadata before the file
        // is opened, so use a writable known syntax for the inert container.
        let encoded_transfer_syntax_uid = if label == "unknown" {
            "1.2.840.10008.1.2.4.80"
        } else {
            transfer_syntax_uid
        };
        support::write_encapsulated_dicom(&path, encoded_transfer_syntax_uid, vec![vec![0]]);
        let entry = support::file_entry(path, transfer_syntax_uid, 1);
        let test_server = TestServer::new(server::router(support::app_state(vec![entry])));

        for endpoint in ["/api/file/0/frame/0", "/api/file/0/frame/0/raw"] {
            let response = test_server.get(endpoint).await;
            assert_eq!(
                response.status_code(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{label} should be rejected by {endpoint}"
            );
            assert_eq!(header(&response, "content-type"), "application/json");
            let body = response.json::<serde_json::Value>();
            assert!(
                body["error"]
                    .as_str()
                    .is_some_and(|message| message.contains(transfer_syntax_uid)),
                "error response should identify transfer syntax {transfer_syntax_uid}: {body}"
            );
        }
    }
}
