use crate::types::{FileEntry, LoadReport, WindowPreset};
use anyhow::{anyhow, Context, Result};
use dicom_object::open_file;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tokio::task;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct DiscoverOptions {
	pub recursive: bool,
}

pub async fn discover(paths: &[PathBuf], options: DiscoverOptions) -> Result<LoadReport> {
	let paths = paths.to_vec();
	task::spawn_blocking(move || discover_blocking(&paths, &options))
		.await
		.context("loader worker panicked")?
}

fn discover_blocking(paths: &[PathBuf], options: &DiscoverOptions) -> Result<LoadReport> {
	let mut candidates = Vec::new();

	for path in paths {
		if path.is_file() {
			candidates.push(path.clone());
			continue;
		}

		if path.is_dir() {
			let mut walker = WalkDir::new(path).follow_links(false);
			if !options.recursive {
				walker = walker.max_depth(1);
			}

			for entry in walker.into_iter().filter_map(|entry| entry.ok()) {
				let entry_path = entry.path();
				if entry_path.is_file() {
					candidates.push(entry_path.to_path_buf());
				}
			}
			continue;
		}
	}

	let processed: Vec<_> = candidates
		.par_iter()
		.map(|candidate| build_entry(candidate))
		.collect();

	let mut files = Vec::new();
	let mut skipped = 0_usize;

	for item in processed {
		match item {
			Ok(Some(entry)) => files.push(entry),
			Ok(None) => skipped += 1,
			Err(_) => skipped += 1,
		}
	}

	if files.is_empty() {
		return Err(anyhow!("dcmview: no valid DICOM files found"));
	}

	files.sort_by(|left, right| left.path.cmp(&right.path));
	for (idx, file) in files.iter_mut().enumerate() {
		file.index = idx;
	}

	Ok(LoadReport {
		files,
		skipped,
		searched_recursive: options.recursive,
	})
}

fn build_entry(path: &Path) -> Result<Option<FileEntry>> {
	let obj = match open_file(path) {
		Ok(obj) => obj,
		Err(_) => return Ok(None),
	};

	let transfer_syntax_uid = read_str(&obj, "TransferSyntaxUID").unwrap_or_else(|| "unknown".to_string());
	let patient_id = read_str(&obj, "PatientID").unwrap_or_default();
	let modality = read_str(&obj, "Modality").unwrap_or_default();
	let study_date = read_str(&obj, "StudyDate").unwrap_or_default();
	let frame_count = read_u32(&obj, "NumberOfFrames").unwrap_or(1);
	let rows = read_u32(&obj, "Rows").unwrap_or(0);
	let columns = read_u32(&obj, "Columns").unwrap_or(0);
	let has_pixels = obj.element_by_name("PixelData").is_ok();
	let default_window = match (read_f64(&obj, "WindowCenter"), read_f64(&obj, "WindowWidth")) {
		(Some(center), Some(width)) => Some(WindowPreset { center, width }),
		_ => None,
	};

	let fallback_label = path
		.file_name()
		.and_then(|name| name.to_str())
		.map(ToString::to_string)
		.unwrap_or_else(|| path.to_string_lossy().to_string());
	let label = build_label(&patient_id, &modality, &study_date, &fallback_label);

	Ok(Some(FileEntry {
		index: 0,
		path: path.to_path_buf(),
		label,
		has_pixels,
		frame_count,
		rows,
		columns,
		transfer_syntax_uid,
		default_window,
		offset_table: None,
	}))
}

fn read_str(obj: &dicom_object::DefaultDicomObject, name: &str) -> Option<String> {
	obj.element_by_name(name)
		.ok()
		.and_then(|element| element.to_str().ok())
		.map(|value| value.to_string())
}

fn read_u32(obj: &dicom_object::DefaultDicomObject, name: &str) -> Option<u32> {
	read_str(obj, name)?.parse::<u32>().ok()
}

fn read_f64(obj: &dicom_object::DefaultDicomObject, name: &str) -> Option<f64> {
	read_str(obj, name)?.parse::<f64>().ok()
}

fn build_label(patient_id: &str, modality: &str, study_date: &str, fallback: &str) -> String {
	let mut fields = Vec::new();
	if !patient_id.is_empty() {
		fields.push(patient_id);
	}
	if !modality.is_empty() {
		fields.push(modality);
	}
	if !study_date.is_empty() {
		fields.push(study_date);
	}

	if fields.is_empty() {
		fallback.to_string()
	} else {
		fields.join(" · ")
	}
}
