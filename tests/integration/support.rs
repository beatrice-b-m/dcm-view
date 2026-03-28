use dcmview::pixels;
use dcmview::server::{now_unix_ms, AppState};
use dcmview::types::FileEntry;
use dicom_core::value::PixelFragmentSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub fn write_encapsulated_dicom(path: &Path, transfer_syntax_uid: &str, fragments: Vec<Vec<u8>>) {
	let frame_count = fragments.len().max(1) as u32;

	let mut obj = InMemDicomObject::from_element_iter([
		DataElement::new(tags::SOP_CLASS_UID, VR::UI, uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION),
		DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, format!("2.25.{}", 100_000 + frame_count)),
		DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from("TEST")),
		DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from("MG")),
		DataElement::new(tags::STUDY_DATE, VR::DA, PrimitiveValue::from("20260101")),
		DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(8_u16)),
		DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(8_u16)),
		DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(7_u16)),
		DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(1_u16)),
		DataElement::new(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, PrimitiveValue::from("MONOCHROME2")),
		DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, PrimitiveValue::from(frame_count.to_string())),
	]);

	obj.put(DataElement::new(
		tags::PIXEL_DATA,
		VR::OB,
		PixelFragmentSequence::new_fragments(fragments),
	));

	let file_object = obj
		.with_meta(
			FileMetaTableBuilder::new()
				.transfer_syntax(transfer_syntax_uid)
				.media_storage_sop_class_uid(uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION)
				.media_storage_sop_instance_uid("2.25.123456789"),
		)
		.expect("build encapsulated file meta");

	file_object.write_to_file(path).expect("write encapsulated DICOM fixture");
}

pub fn file_entry(path: PathBuf, transfer_syntax_uid: &str, frame_count: u32) -> FileEntry {
	FileEntry {
		index: 0,
		path,
		label: "fixture".to_string(),
		has_pixels: true,
		frame_count,
		rows: 16,
		columns: 16,
		transfer_syntax_uid: transfer_syntax_uid.to_string(),
		default_window: None,
	}
}


pub fn app_state(files: Vec<FileEntry>) -> AppState {
	AppState {
		files: Arc::new(files),
		pixel_cache: pixels::new_cache(),
		tag_cache: Arc::new(Mutex::new(HashMap::new())),
		tunnel_info: None,
		tunnel_handle: None,
		server_start: Instant::now(),
		server_start_ms: now_unix_ms(),
		last_request: Arc::new(AtomicU64::new(now_unix_ms())),
	}
}

pub fn write_uncompressed_u16_dicom(
	path: &Path,
	transfer_syntax_uid: &str,
	rows: u16,
	columns: u16,
	frames: Vec<u16>,
	window_center: Option<&str>,
	window_width: Option<&str>,
) {
	let pixels_per_frame = rows as usize * columns as usize;
	let frame_count = (frames.len() / pixels_per_frame).max(1) as u32;

	let big_endian = transfer_syntax_uid == "1.2.840.10008.1.2.2";
	let mut pixel_bytes = Vec::with_capacity(frames.len() * 2);
	for sample in &frames {
		let bytes = if big_endian {
			sample.to_be_bytes()
		} else {
			sample.to_le_bytes()
		};
		pixel_bytes.extend_from_slice(&bytes);
	}

	let mut obj = InMemDicomObject::from_element_iter([
		DataElement::new(tags::SOP_CLASS_UID, VR::UI, uids::CT_IMAGE_STORAGE),
		DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, format!("2.25.{}", 300_000 + frame_count)),
		DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from("UNCOMP")),
		DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from("CT")),
		DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(rows)),
		DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(columns)),
		DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(16_u16)),
		DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(15_u16)),
		DataElement::new(tags::PIXEL_REPRESENTATION, VR::US, PrimitiveValue::from(0_u16)),
		DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(1_u16)),
		DataElement::new(tags::PHOTOMETRIC_INTERPRETATION, VR::CS, PrimitiveValue::from("MONOCHROME2")),
		DataElement::new(tags::NUMBER_OF_FRAMES, VR::IS, PrimitiveValue::from(frame_count.to_string())),
		DataElement::new(tags::PIXEL_DATA, VR::OW, PrimitiveValue::from(pixel_bytes)),
	]);

	if let Some(center) = window_center {
		obj.put(DataElement::new(tags::WINDOW_CENTER, VR::DS, PrimitiveValue::from(center)));
	}
	if let Some(width) = window_width {
		obj.put(DataElement::new(tags::WINDOW_WIDTH, VR::DS, PrimitiveValue::from(width)));
	}

	let file_object = obj
		.with_meta(
			FileMetaTableBuilder::new()
				.transfer_syntax(transfer_syntax_uid)
				.media_storage_sop_class_uid(uids::CT_IMAGE_STORAGE)
				.media_storage_sop_instance_uid("2.25.987654321"),
		)
		.expect("build uncompressed file meta");

	file_object
		.write_to_file(path)
		.expect("write uncompressed DICOM fixture");
}