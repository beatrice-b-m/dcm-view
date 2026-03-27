use super::support;
use axum_test::TestServer;
use dcmview::server;
use dcmview::types::FileEntry;
use dicom_core::value::DataSetSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use serde_json::Value;
use tempfile::tempdir;

#[tokio::test]
async fn serializes_sequences_and_binary_values_without_leaking_raw_pixel_data() {
	let dir = tempdir().expect("temp dir");
	let path = dir.path().join("tags-sequence.dcm");
	write_tag_fixture(&path);

	let file = FileEntry {
		index: 0,
		path: path.clone(),
		label: "tag fixture".to_string(),
		has_pixels: true,
		frame_count: 1,
		rows: 8,
		columns: 8,
		transfer_syntax_uid: "1.2.840.10008.1.2.1".to_string(),
		default_window: None,
		offset_table: None,
	};

	let app = server::router(support::app_state(vec![file]));
	let test_server = TestServer::new(app).expect("test server");

	let response = test_server.get("/api/file/0/tags").await;
	response.assert_status_ok();
	let payload: Value = response.json();
	let rows = payload.as_array().expect("tag list array");
	assert!(!rows.is_empty());

	let pixel_data_row = rows
		.iter()
		.find(|row| row["tag"] == "(7FE0,0010)")
		.expect("pixel data row");
	assert_eq!(pixel_data_row["value"]["type"], "binary");
	assert_eq!(pixel_data_row["value"]["length"], 512);

	let sequence_row = rows
		.iter()
		.find(|row| row["vr"] == "SQ")
		.expect("sequence row");
	assert_eq!(sequence_row["value"]["type"], "sequence");
	assert_eq!(
		sequence_row["value"]["items"].as_array().expect("sequence items").len(),
		1
	);

	let tags = rows
		.iter()
		.map(|row| row["tag"].as_str().unwrap_or_default().to_string())
		.collect::<Vec<_>>();
	let mut sorted = tags.clone();
	sorted.sort();
	assert_eq!(tags, sorted, "tags must be ordered by ascending tag number");
}

fn write_tag_fixture(path: &std::path::Path) {
	let sequence_item = InMemDicomObject::from_element_iter([
		DataElement::new(tags::CODE_VALUE, VR::SH, PrimitiveValue::from("T-04000")),
		DataElement::new(tags::CODE_MEANING, VR::LO, PrimitiveValue::from("Breast")),
	]);

	let mut obj = InMemDicomObject::from_element_iter([
		DataElement::new(tags::SOP_CLASS_UID, VR::UI, uids::CT_IMAGE_STORAGE),
		DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, PrimitiveValue::from("2.25.123123123")),
		DataElement::new(tags::PATIENT_NAME, VR::PN, PrimitiveValue::from("Doe^Jane")),
		DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(8_u16)),
		DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(8_u16)),
		DataElement::new(tags::PIXEL_DATA, VR::OB, PrimitiveValue::from(vec![7_u8; 512])),
	]);

	obj.put(DataElement::new(
		tags::ANATOMIC_REGION_SEQUENCE,
		VR::SQ,
		DataSetSequence::from(vec![sequence_item]),
	));

	let file_object = obj
		.with_meta(
			FileMetaTableBuilder::new()
				.transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN)
				.media_storage_sop_class_uid(uids::CT_IMAGE_STORAGE)
				.media_storage_sop_instance_uid("2.25.123123123"),
		)
		.expect("build meta table");
	file_object.write_to_file(path).expect("write tag fixture");
}
