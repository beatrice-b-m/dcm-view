use dcmview::loader::{self, DiscoverOptions};
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

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

	let report = loader::discover(
		&[dir.path().to_path_buf()],
		DiscoverOptions { recursive: true },
	)
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
async fn respects_no_recursive_for_directory_inputs() {
	let dir = tempdir().expect("temp dir");
	let nested = dir.path().join("nested");
	fs::create_dir_all(&nested).expect("nested dir");

	let top = dir.path().join("top.dcm");
	let nested_file = nested.join("nested.dcm");
	write_test_dicom(&top, "TOP", "CT", "20260101", 2, true);
	write_test_dicom(&nested_file, "NESTED", "CT", "20260101", 2, true);

	let report = loader::discover(
		&[dir.path().to_path_buf()],
		DiscoverOptions { recursive: false },
	)
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

	let error = loader::discover(
		&[dir.path().to_path_buf()],
		DiscoverOptions { recursive: true },
	)
	.await
	.expect_err("loader should fail when no DICOM files exist");

	assert!(
		error.to_string().contains("no valid DICOM files"),
		"error should explain why startup fails"
	);
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
		DataElement::new(tags::SOP_CLASS_UID, VR::UI, uids::COMPUTED_RADIOGRAPHY_IMAGE_STORAGE),
		DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, format!("2.25.{}", 10_000 + frame_count)),
		DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from(patient_id)),
		DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from(modality)),
		DataElement::new(tags::STUDY_DATE, VR::DA, PrimitiveValue::from(study_date)),
		DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, PrimitiveValue::from(frame_count.to_string())),
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

	file_object.write_to_file(path).expect("write DICOM fixture");
}
