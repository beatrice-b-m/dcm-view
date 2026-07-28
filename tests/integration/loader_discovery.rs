use dcmview::loader::{self, DiscoverOptions};
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use std::fs;
use std::io::{Seek, Write};
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;
use tokio::time::timeout;

const DISCOVERY_TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn discover_options(recursive: bool) -> DiscoverOptions {
    DiscoverOptions {
        recursive,
        filters: Vec::new(),
    }
}

#[tokio::test]
async fn discovers_valid_files_and_tracks_skips() {
    let dir = tempdir().expect("temp dir");
    let nested = dir.path().join("nested");
    fs::create_dir_all(&nested).expect("nested dir");

    let first = dir.path().join("first.dcm");
    let second = nested.join("second.dcm");
    let invalid = dir.path().join("not-dicom.bin");

    write_test_dicom(&first, "P1", "MG", "20260101", 1, true);
    write_test_dicom(&second, "P2", "MR", "20260102", 4, false);
    fs::write(&invalid, b"not a dicom file").expect("invalid file");

    let report = loader::discover(&[dir.path().to_path_buf()], discover_options(true))
        .await
        .expect("discovery should succeed");

    assert_eq!(report.files.len(), 2, "expected both DICOM files");
    assert_eq!(report.skipped, 1, "expected one skipped non-DICOM file");
    assert!(report.searched_recursive);

    let first_loaded = &report.files[0];
    assert_eq!(first_loaded.index, 0);
    assert!(first_loaded.label.contains("P1") || first_loaded.label.contains("P2"));
    assert_eq!(
        first_loaded.transfer_syntax_uid,
        uids::EXPLICIT_VR_LITTLE_ENDIAN,
        "transfer syntax should come from file meta"
    );
}

#[tokio::test]
async fn progressive_discovery_applies_backpressure_and_reports_each_disposition() {
    let dir = tempdir().expect("temp dir");
    let accepted = dir.path().join("accepted.dcm");
    let filtered = dir.path().join("filtered.dcm");
    let invalid = dir.path().join("not-dicom.bin");
    write_test_dicom(&accepted, "PAT-CT", "CT", "20260101", 1, true);
    write_test_dicom(&filtered, "PAT-MR", "MR", "20260102", 1, true);
    fs::write(&invalid, b"not a dicom file").expect("invalid file");

    // A capacity of one forces the blocking discovery workers to wait for the
    // async consumer instead of buffering an entire large-directory scan.
    let (events_tx, mut events_rx) = mpsc::channel(1);
    let scan_path = dir.path().to_path_buf();
    let scan = tokio::spawn(async move {
        loader::discover_progressive(
            &[scan_path],
            DiscoverOptions {
                recursive: true,
                filters: vec!["modality=CT".parse().expect("filter parses")],
            },
            events_tx,
        )
        .await
    });

    let mut accepted_events = 0;
    let mut skipped_events = 0;
    let mut filtered_events = 0;
    while let Some(event) = events_rx.recv().await {
        match event {
            loader::DiscoveryEvent::File(file) => {
                accepted_events += 1;
                assert_eq!(file.modality, "CT");
            }
            loader::DiscoveryEvent::Skipped => skipped_events += 1,
            loader::DiscoveryEvent::Filtered => filtered_events += 1,
        }
    }

    let report = scan
        .await
        .expect("progressive scan task should finish")
        .expect("progressive discovery should succeed");
    assert_eq!(accepted_events, 1);
    assert_eq!(skipped_events, 1);
    assert_eq!(filtered_events, 1);
    assert_eq!(report.files_found, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.filtered, 1);
}

#[tokio::test]
async fn progressive_discovery_stops_before_work_when_pre_cancelled() {
    let dir = tempdir().expect("temp dir");
    copy_golden_discovery_fixtures(dir.path(), 4);

    let cancellation = loader::DiscoveryCancellation::new();
    cancellation.cancel();
    let (events_tx, mut events_rx) = mpsc::channel(1);

    let result = timeout(
        DISCOVERY_TEST_TIMEOUT,
        loader::discover_progressive_with_cancellation(
            &[dir.path().to_path_buf()],
            discover_options(true),
            events_tx,
            cancellation,
        ),
    )
    .await
    .expect("pre-cancelled discovery should terminate promptly");

    assert_discovery_cancelled(
        result.expect_err("pre-cancelled discovery should fail"),
        loader::DiscoveryCancellationReason::Requested,
    );
    assert!(
        events_rx.try_recv().is_err(),
        "pre-cancelled discovery must not emit events"
    );
}

#[tokio::test]
async fn progressive_discovery_stops_when_event_receiver_is_closed() {
    let dir = tempdir().expect("temp dir");
    copy_golden_discovery_fixtures(dir.path(), 4);

    let (events_tx, events_rx) = mpsc::channel(1);
    drop(events_rx);

    let result = timeout(
        DISCOVERY_TEST_TIMEOUT,
        loader::discover_progressive_with_cancellation(
            &[dir.path().to_path_buf()],
            discover_options(true),
            events_tx,
            loader::DiscoveryCancellation::new(),
        ),
    )
    .await
    .expect("receiver-closed discovery should terminate promptly");

    assert_discovery_cancelled(
        result.expect_err("receiver-closed discovery should fail"),
        loader::DiscoveryCancellationReason::EventReceiverClosed,
    );
}

#[tokio::test]
async fn progressive_discovery_cancels_while_bounded_channel_is_full() {
    let dir = tempdir().expect("temp dir");
    copy_golden_discovery_fixtures(dir.path(), 64);

    let cancellation = loader::DiscoveryCancellation::new();
    let scan_cancellation = cancellation.clone();
    let scan_path = dir.path().to_path_buf();
    let (events_tx, mut events_rx) = mpsc::channel(1);
    let scan = tokio::spawn(async move {
        loader::discover_progressive_with_cancellation(
            &[scan_path],
            discover_options(true),
            events_tx,
            scan_cancellation,
        )
        .await
    });

    timeout(DISCOVERY_TEST_TIMEOUT, events_rx.recv())
        .await
        .expect("discovery should emit its first event")
        .expect("event channel should remain open");
    timeout(DISCOVERY_TEST_TIMEOUT, async {
        while events_rx.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("bounded event channel should fill");

    cancellation.cancel();
    let result = timeout(DISCOVERY_TEST_TIMEOUT, scan)
        .await
        .expect("mid-stream cancellation should terminate promptly")
        .expect("progressive scan task should not panic");

    assert_discovery_cancelled(
        result.expect_err("cancelled discovery should fail"),
        loader::DiscoveryCancellationReason::Requested,
    );
}

#[tokio::test]
async fn respects_no_recursive_for_directory_inputs() {
    let dir = tempdir().expect("temp dir");
    let nested = dir.path().join("nested");
    fs::create_dir_all(&nested).expect("nested dir");

    let top = dir.path().join("top.dcm");
    let nested_file = nested.join("nested.dcm");
    write_test_dicom(&top, "TOP", "CT", "20260101", 2, true);
    write_test_dicom(&nested_file, "NESTED", "CT", "20260101", 2, true);

    let report = loader::discover(&[dir.path().to_path_buf()], discover_options(false))
        .await
        .expect("discovery should succeed");

    assert_eq!(report.files.len(), 1, "nested file must be excluded");
    assert_eq!(report.files[0].path, top);
    assert!(!report.searched_recursive);
}

#[tokio::test]
async fn errors_when_no_valid_files_found() {
    let dir = tempdir().expect("temp dir");
    let invalid = dir.path().join("invalid.txt");
    fs::write(&invalid, b"plain text").expect("invalid file");

    let error = loader::discover(&[dir.path().to_path_buf()], discover_options(true))
        .await
        .expect_err("loader should fail when no DICOM files exist");

    assert!(
        error.to_string().contains("no valid DICOM files"),
        "error should explain why startup fails"
    );
}

#[tokio::test]
async fn filters_matching_subset_by_metadata_field() {
    let dir = tempdir().expect("temp dir");
    let ct = dir.path().join("ct.dcm");
    let mr = dir.path().join("mr.dcm");
    write_test_dicom(&ct, "PAT-CT", "CT", "20260101", 1, true);
    write_test_dicom(&mr, "PAT-MR", "MR", "20260102", 1, true);

    let report = loader::discover(
        &[dir.path().to_path_buf()],
        DiscoverOptions {
            recursive: true,
            filters: vec!["modality=MR".parse().expect("filter parses")],
        },
    )
    .await
    .expect("filtered discovery should succeed");

    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].patient_id, "PAT-MR");
    assert_eq!(report.filtered, 1);
}

#[tokio::test]
async fn accepts_case_insensitive_filter_field_names() {
    let dir = tempdir().expect("temp dir");
    let ct = dir.path().join("ct.dcm");
    let mr = dir.path().join("mr.dcm");
    write_test_dicom(&ct, "PAT-CT", "CT", "20260101", 1, true);
    write_test_dicom(&mr, "PAT-MR", "MR", "20260102", 1, true);

    let report = loader::discover(
        &[dir.path().to_path_buf()],
        DiscoverOptions {
            recursive: true,
            filters: vec!["Modality=MR".parse().expect("filter parses")],
        },
    )
    .await
    .expect("filtered discovery should succeed");

    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].patient_id, "PAT-MR");
}

#[tokio::test]
async fn filters_and_multiple_terms_together() {
    let dir = tempdir().expect("temp dir");
    let first = dir.path().join("first.dcm");
    let second = dir.path().join("second.dcm");
    write_test_dicom(&first, "PAT-001", "MR", "20260101", 1, true);
    write_test_dicom(&second, "PAT-002", "MR", "20260102", 1, true);

    let report = loader::discover(
        &[dir.path().to_path_buf()],
        DiscoverOptions {
            recursive: true,
            filters: vec![
                "modality=mr".parse().expect("modality filter parses"),
                "patient_id=002".parse().expect("patient filter parses"),
            ],
        },
    )
    .await
    .expect("filtered discovery should succeed");

    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].patient_id, "PAT-002");
    assert_eq!(report.filtered, 1);
}

#[tokio::test]
async fn filters_matching_nothing_error_after_valid_files_are_filtered() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("ct.dcm");
    write_test_dicom(&path, "PAT-CT", "CT", "20260101", 1, true);

    let error = loader::discover(
        &[dir.path().to_path_buf()],
        DiscoverOptions {
            recursive: true,
            filters: vec!["modality=MR".parse().expect("filter parses")],
        },
    )
    .await
    .expect_err("all-filtered discovery should fail");

    assert!(error.to_string().contains("no valid DICOM files"));
}

#[tokio::test]
async fn ignores_pixel_data_byte_pattern_in_file_preamble() {
    let dir = tempdir().expect("temp dir");
    let path = dir.path().join("no-pixels-preamble-pattern.dcm");
    write_test_dicom(&path, "PAT-SR", "SR", "20260101", 1, false);

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open fixture for preamble edit");
    file.seek(std::io::SeekFrom::Start(0))
        .expect("seek preamble");
    file.write_all(&[0xe0, 0x7f, 0x10, 0x00])
        .expect("write preamble pattern");

    let report = loader::discover(&[path], discover_options(true))
        .await
        .expect("discovery should succeed");

    assert_eq!(report.files.len(), 1);
    assert!(!report.files[0].has_pixels);
}

#[test]
fn rejects_unknown_filter_field() {
    let error = "unknown=value"
        .parse::<loader::ScanFilter>()
        .expect_err("unknown fields should be rejected");

    assert!(error.contains("FIELD is one of"));
    assert!(error.contains("patient_id"));
    assert!(error.contains("modality"));
}

fn assert_discovery_cancelled(
    error: anyhow::Error,
    expected_reason: loader::DiscoveryCancellationReason,
) {
    assert!(
        loader::is_discovery_cancelled(&error),
        "cancellation helper should recognize typed error: {error:#}"
    );
    assert_eq!(
        loader::discovery_cancellation_reason(&error),
        Some(expected_reason)
    );
    assert_eq!(
        error
            .downcast_ref::<loader::DiscoveryCancelled>()
            .expect("typed cancellation error")
            .reason(),
        expected_reason
    );
}

fn copy_golden_discovery_fixtures(directory: &Path, count: usize) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden-no-pixels-sr.dcm");

    for index in 0..count {
        fs::copy(&fixture, directory.join(format!("fixture-{index:03}.dcm")))
            .expect("copy generated DICOM fixture");
    }
}

fn write_test_dicom(
    path: &Path,
    patient_id: &str,
    modality: &str,
    study_date: &str,
    frame_count: u32,
    has_pixels: bool,
) {
    let mut obj = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::COMPUTED_RADIOGRAPHY_IMAGE_STORAGE,
        ),
        DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            format!("2.25.{}", 10_000 + frame_count),
        ),
        DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from(patient_id)),
        DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from(modality)),
        DataElement::new(tags::STUDY_DATE, VR::DA, PrimitiveValue::from(study_date)),
        DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(frame_count.to_string()),
        ),
        DataElement::new(tags::WINDOW_CENTER, VR::DS, PrimitiveValue::from("40")),
        DataElement::new(tags::WINDOW_WIDTH, VR::DS, PrimitiveValue::from("80")),
    ]);

    if has_pixels {
        obj.put(DataElement::new(
            tags::PIXEL_DATA,
            VR::OB,
            PrimitiveValue::from(vec![0_u8; 16 * 16]),
        ));
    }

    let file_object = obj
        .with_meta(
            FileMetaTableBuilder::new()
                .transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
                .media_storage_sop_class_uid(uids::COMPUTED_RADIOGRAPHY_IMAGE_STORAGE)
                .media_storage_sop_instance_uid(format!("2.25.{}", 20_000 + frame_count)),
        )
        .expect("build file meta");

    file_object
        .write_to_file(path)
        .expect("write DICOM fixture");
}
