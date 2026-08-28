use dcmview::annotations::AnnotationStore;
use dcmview::server::{AppState, FileRegistry};
use dcmview::types::FileEntry;
use dicom_core::value::PixelFragmentSequence;
use dicom_core::{DataElement, PrimitiveValue, VR};
use dicom_dictionary_std::{tags, uids};
use dicom_object::{meta::FileMetaTableBuilder, InMemDicomObject};
use image::{GrayImage, Luma};
use std::path::{Path, PathBuf};

pub fn write_encapsulated_dicom(path: &Path, transfer_syntax_uid: &str, fragments: Vec<Vec<u8>>) {
    let frame_count = fragments.len().max(1) as u32;

    let mut obj = InMemDicomObject::from_element_iter([
        DataElement::new(
            tags::SOP_CLASS_UID,
            VR::UI,
            uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
        ),
        DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            format!("2.25.{}", 100_000 + frame_count),
        ),
        DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from("TEST")),
        DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from("MG")),
        DataElement::new(tags::STUDY_DATE, VR::DA, PrimitiveValue::from("20260101")),
        DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(8_u16)),
        DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(8_u16)),
        DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(7_u16)),
        DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0_u16),
        ),
        DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(1_u16)),
        DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            PrimitiveValue::from("MONOCHROME2"),
        ),
        DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(frame_count.to_string()),
        ),
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
                .media_storage_sop_class_uid(
                    uids::DIGITAL_MAMMOGRAPHY_X_RAY_IMAGE_STORAGE_FOR_PRESENTATION,
                )
                .media_storage_sop_instance_uid("2.25.123456789"),
        )
        .expect("build encapsulated file meta");

    file_object
        .write_to_file(path)
        .expect("write encapsulated DICOM fixture");
}

pub fn file_entry(path: PathBuf, transfer_syntax_uid: &str, frame_count: u32) -> FileEntry {
    FileEntry {
        index: 0,
        path,
        label: "fixture".to_string(),
        patient_id: "TEST".to_string(),
        patient_name: "Test^Patient".to_string(),
        study_instance_uid: "1.2.826.0.1.3680043.10.100.1".to_string(),
        study_date: "20260101".to_string(),
        study_description: "Fixture study".to_string(),
        series_instance_uid: "1.2.826.0.1.3680043.10.100.2".to_string(),
        series_number: "1".to_string(),
        series_description: "Fixture series".to_string(),
        modality: "OT".to_string(),
        instance_number: "1".to_string(),
        sop_instance_uid: "1.2.826.0.1.3680043.10.100.3".to_string(),
        sop_class_uid: "1.2.840.10008.5.1.4.1.1.2".to_string(),
        has_pixels: true,
        frame_count,
        rows: 16,
        columns: 16,
        bits_allocated: 16,
        pixel_representation: 0,
        samples_per_pixel: 1,
        photometric_interpretation: "MONOCHROME2".to_string(),
        rescale_slope: 1.0,
        rescale_intercept: 0.0,
        transfer_syntax_uid: transfer_syntax_uid.to_string(),
        default_window: None,
    }
}

pub fn grayscale_jpeg_fragment_16x16(seed: u8) -> Vec<u8> {
    let image = GrayImage::from_fn(16, 16, |x, y| {
        let value = seed
            .wrapping_add((x as u8).wrapping_mul(7))
            .wrapping_add((y as u8).wrapping_mul(11));
        Luma([value])
    });
    let mut encoded = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90);
    encoder
        .encode_image(&image)
        .expect("encode grayscale jpeg fixture");
    encoded
}

pub fn app_state(files: Vec<FileEntry>) -> AppState {
    app_state_with_registry(FileRegistry::from_files(files))
}

pub fn app_state_with_annotations(files: Vec<FileEntry>, annotations: AnnotationStore) -> AppState {
    AppState::new(FileRegistry::from_files(files), annotations)
}

pub fn app_state_with_registry(registry: FileRegistry) -> AppState {
    AppState::new(registry, AnnotationStore::empty())
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
    write_uncompressed_u16_dicom_with_photometric(
        path,
        transfer_syntax_uid,
        (rows, columns),
        frames,
        "MONOCHROME2",
        window_center,
        window_width,
    );
}

pub fn write_uncompressed_u16_dicom_with_photometric(
    path: &Path,
    transfer_syntax_uid: &str,
    dimensions: (u16, u16),
    frames: Vec<u16>,
    photometric_interpretation: &str,
    window_center: Option<&str>,
    window_width: Option<&str>,
) {
    let (rows, columns) = dimensions;
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
        DataElement::new(
            tags::SOP_INSTANCE_UID,
            VR::UI,
            format!("2.25.{}", 300_000 + frame_count),
        ),
        DataElement::new(tags::PATIENT_ID, VR::LO, PrimitiveValue::from("UNCOMP")),
        DataElement::new(tags::MODALITY, VR::CS, PrimitiveValue::from("CT")),
        DataElement::new(tags::ROWS, VR::US, PrimitiveValue::from(rows)),
        DataElement::new(tags::COLUMNS, VR::US, PrimitiveValue::from(columns)),
        DataElement::new(tags::BITS_ALLOCATED, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::BITS_STORED, VR::US, PrimitiveValue::from(16_u16)),
        DataElement::new(tags::HIGH_BIT, VR::US, PrimitiveValue::from(15_u16)),
        DataElement::new(
            tags::PIXEL_REPRESENTATION,
            VR::US,
            PrimitiveValue::from(0_u16),
        ),
        DataElement::new(tags::SAMPLES_PER_PIXEL, VR::US, PrimitiveValue::from(1_u16)),
        DataElement::new(
            tags::PHOTOMETRIC_INTERPRETATION,
            VR::CS,
            PrimitiveValue::from(photometric_interpretation),
        ),
        DataElement::new(
            tags::NUMBER_OF_FRAMES,
            VR::IS,
            PrimitiveValue::from(frame_count.to_string()),
        ),
        DataElement::new(tags::PIXEL_DATA, VR::OW, PrimitiveValue::from(pixel_bytes)),
    ]);

    if let Some(center) = window_center {
        obj.put(DataElement::new(
            tags::WINDOW_CENTER,
            VR::DS,
            PrimitiveValue::from(center),
        ));
    }
    if let Some(width) = window_width {
        obj.put(DataElement::new(
            tags::WINDOW_WIDTH,
            VR::DS,
            PrimitiveValue::from(width),
        ));
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
