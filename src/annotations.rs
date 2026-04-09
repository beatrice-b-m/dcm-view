use crate::types::FileEntry;
use anyhow::{anyhow, bail, Context, Result};
use csv::{ReaderBuilder, StringRecord};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

const REQUIRED_COLUMNS: [&str; 4] = ["anon_dicom_path", "num_ROI", "ROI_coords", "ROI_frames"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbedRoiAnnotations {
	pub num_roi: usize,
	pub roi_coords: Vec<[u32; 4]>,
	pub roi_frames: Vec<Vec<u32>>,
}

impl EmbedRoiAnnotations {
	pub fn empty() -> Self {
		Self {
			num_roi: 0,
			roi_coords: Vec::new(),
			roi_frames: Vec::new(),
		}
	}
}

#[derive(Debug, Clone)]
struct ParsedAnnotationRow {
	row_number: usize,
	normalized_path: String,
	annotations: EmbedRoiAnnotations,
}

pub type AnnotationIndexMap = HashMap<usize, EmbedRoiAnnotations>;

pub fn load_annotations_for_files(csv_path: &Path, files: &[FileEntry]) -> Result<AnnotationIndexMap> {
	let rows = parse_rows(csv_path)?;
	let file_lookup = build_file_lookup(files)?;

	let mut annotations_by_file = HashMap::new();
	for row in rows {
		if let Some(file_targets) = file_lookup.get(&row.normalized_path) {
			for &(file_index, frame_count) in file_targets {
				validate_frames_in_range(&row.annotations, frame_count, row.row_number)?;
				annotations_by_file.insert(file_index, row.annotations.clone());
			}
		}
	}

	Ok(annotations_by_file)
}

fn parse_rows(csv_path: &Path) -> Result<Vec<ParsedAnnotationRow>> {
	let mut reader = ReaderBuilder::new()
		.flexible(false)
		.from_path(csv_path)
		.with_context(|| format!("failed to open annotations CSV: {}", csv_path.display()))?;

	let headers = reader
		.headers()
		.with_context(|| format!("failed to read annotations CSV header: {}", csv_path.display()))?
		.clone();
	let indexes = required_column_indexes(&headers)?;

	let mut rows = Vec::new();
	let mut seen_paths = HashMap::<String, usize>::new();

	for (idx, row_result) in reader.records().enumerate() {
		let row_number = idx + 2; // Header is line 1.
		let row = row_result
			.with_context(|| format!("annotations CSV row {row_number} could not be parsed"))?;
		let parsed = parse_row(&row, &indexes, row_number)?;

		if let Some(previous_row) = seen_paths.insert(parsed.normalized_path.clone(), parsed.row_number) {
			bail!(
				"annotations CSV row {}: duplicate anon_dicom_path (already seen at row {})",
				parsed.row_number,
				previous_row
			);
		}

		rows.push(parsed);
	}

	Ok(rows)
}

fn required_column_indexes(headers: &StringRecord) -> Result<HashMap<&'static str, usize>> {
	let mut indexes = HashMap::new();
	for column in REQUIRED_COLUMNS {
		let idx = headers
			.iter()
			.position(|candidate| candidate == column)
			.ok_or_else(|| anyhow!("annotations CSV missing required column `{column}`"))?;
		indexes.insert(column, idx);
	}
	Ok(indexes)
}

fn parse_row(row: &StringRecord, indexes: &HashMap<&'static str, usize>, row_number: usize) -> Result<ParsedAnnotationRow> {
	let raw_path = read_column(row, indexes, "anon_dicom_path", row_number)?;
	let normalized_path = normalize_path(Path::new(raw_path.trim()));
	if normalized_path.is_empty() {
		bail!("annotations CSV row {row_number}: anon_dicom_path must not be empty");
	}

	let raw_num_roi = read_column(row, indexes, "num_ROI", row_number)?;
	let num_roi = raw_num_roi
		.trim()
		.parse::<usize>()
		.map_err(|error| anyhow!("annotations CSV row {row_number}: num_ROI must be an integer: {error}"))?;

	let raw_roi_coords = read_column(row, indexes, "ROI_coords", row_number)?;
	let roi_coords = parse_roi_coords(raw_roi_coords.trim(), row_number)?;
	if roi_coords.len() != num_roi {
		bail!(
			"annotations CSV row {row_number}: len(ROI_coords) must equal num_ROI ({} != {})",
			roi_coords.len(),
			num_roi
		);
	}

	let raw_roi_frames = read_column(row, indexes, "ROI_frames", row_number)?;
	let roi_frames = parse_roi_frames(raw_roi_frames.trim(), row_number)?;
	if !roi_frames.is_empty() && roi_frames.len() != num_roi {
		bail!(
			"annotations CSV row {row_number}: len(ROI_frames) must equal num_ROI when ROI_frames is not empty ({} != {})",
			roi_frames.len(),
			num_roi
		);
	}

	Ok(ParsedAnnotationRow {
		row_number,
		normalized_path,
		annotations: EmbedRoiAnnotations {
			num_roi,
			roi_coords,
			roi_frames,
		},
	})
}

fn read_column<'a>(
	row: &'a StringRecord,
	indexes: &HashMap<&'static str, usize>,
	column: &'static str,
	row_number: usize,
) -> Result<&'a str> {
	let idx = indexes
		.get(column)
		.copied()
		.ok_or_else(|| anyhow!("annotations CSV row {row_number}: missing required column `{column}`"))?;
	row
		.get(idx)
		.ok_or_else(|| anyhow!("annotations CSV row {row_number}: missing value for `{column}`"))
}

fn parse_roi_coords(raw: &str, row_number: usize) -> Result<Vec<[u32; 4]>> {
	let parsed: Vec<Vec<u32>> = serde_json::from_str(raw).map_err(|error| {
		anyhow!(
			"annotations CSV row {row_number}: ROI_coords must be a JSON list of [ymin, xmin, ymax, xmax] arrays: {error}"
		)
	})?;

	let mut coords = Vec::with_capacity(parsed.len());
	for (idx, coord) in parsed.into_iter().enumerate() {
		if coord.len() != 4 {
			bail!(
				"annotations CSV row {row_number}: ROI_coords[{idx}] must contain exactly 4 integers [ymin, xmin, ymax, xmax]"
			);
		}
		coords.push([coord[0], coord[1], coord[2], coord[3]]);
	}

	Ok(coords)
}

fn parse_roi_frames(raw: &str, row_number: usize) -> Result<Vec<Vec<u32>>> {
	serde_json::from_str(raw).map_err(|error| {
		anyhow!(
			"annotations CSV row {row_number}: ROI_frames must be a JSON list of frame-index lists: {error}"
		)
	})
}

fn build_file_lookup(files: &[FileEntry]) -> Result<HashMap<String, Vec<(usize, u32)>>> {
	let mut lookup = HashMap::<String, Vec<(usize, u32)>>::new();
	for file in files {
		let normalized = normalize_path(&file.path);
		if normalized.is_empty() {
			bail!(
				"annotations: loaded DICOM path normalized to empty string: {}",
				file.path.display()
			);
		}
		lookup.entry(normalized).or_default().push((file.index, file.frame_count));
	}
	Ok(lookup)
}

fn validate_frames_in_range(annotations: &EmbedRoiAnnotations, frame_count: u32, row_number: usize) -> Result<()> {
	if annotations.roi_frames.is_empty() {
		return Ok(());
	}

	for (roi_idx, frames) in annotations.roi_frames.iter().enumerate() {
		for frame in frames {
			if *frame >= frame_count {
				bail!(
					"annotations CSV row {row_number}: ROI_frames[{roi_idx}] contains frame {frame}, but matched DICOM has {} frame(s)",
					frame_count
				);
			}
		}
	}

	Ok(())
}

fn normalize_path(path: &Path) -> String {
	let mut normalized = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				normalized.pop();
			}
			Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
			Component::RootDir => normalized.push(component.as_os_str()),
			Component::Normal(part) => normalized.push(part),
		}
	}
	normalized.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
	use super::{load_annotations_for_files, EmbedRoiAnnotations};
	use crate::types::FileEntry;
	use std::fs;
	use std::path::{Path, PathBuf};
	use tempfile::tempdir;

	#[test]
	fn maps_valid_annotations_to_matching_files() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		let matched_file = dir.path().join("matched.dcm");
		let unmatched_file = dir.path().join("unmatched.dcm");

		write_csv(
			&csv_path,
			&format!(
				"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n{matched},2,\"[[10,20,30,40],[50,60,70,80]]\",\"[[0,1],[2]]\"\n",
				matched = matched_file.display(),
			),
		);

		let files = vec![
			file_entry(0, matched_file.clone(), 3),
			file_entry(1, unmatched_file.clone(), 1),
		];
		let mapped = load_annotations_for_files(&csv_path, &files).expect("annotations should parse");

		assert_eq!(mapped.len(), 1);
		assert_eq!(
			mapped.get(&0),
			Some(&EmbedRoiAnnotations {
				num_roi: 2,
				roi_coords: vec![[10, 20, 30, 40], [50, 60, 70, 80]],
				roi_frames: vec![vec![0, 1], vec![2]],
			})
		);
		assert!(!mapped.contains_key(&1));
	}

	#[test]
	fn accepts_empty_roi_frames_for_non_dbt_rows() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		let matched_file = dir.path().join("matched.dcm");

		write_csv(
			&csv_path,
			&format!(
				"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n{matched},1,\"[[1,2,3,4]]\",\"[]\"\n",
				matched = matched_file.display(),
			),
		);

		let files = vec![file_entry(0, matched_file.clone(), 42)];
		let mapped = load_annotations_for_files(&csv_path, &files).expect("annotations should parse");
		assert_eq!(mapped.get(&0).map(|value| value.roi_frames.clone()), Some(vec![]));
	}

	#[test]
	fn errors_when_required_column_is_missing() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		write_csv(&csv_path, "anon_dicom_path,num_ROI,ROI_coords\n/path/one.dcm,1,\"[[1,2,3,4]]\"\n");

		let error = load_annotations_for_files(&csv_path, &[]).expect_err("missing header should fail");
		assert!(error.to_string().contains("missing required column `ROI_frames`"));
	}

	#[test]
	fn errors_when_num_roi_and_coords_count_do_not_align() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		write_csv(
			&csv_path,
			"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n/path/one.dcm,2,\"[[1,2,3,4]]\",\"[]\"\n",
		);

		let error = load_annotations_for_files(&csv_path, &[]).expect_err("mismatched ROI count should fail");
		assert!(error.to_string().contains("len(ROI_coords) must equal num_ROI"));
	}

	#[test]
	fn errors_when_roi_frames_length_does_not_match_num_roi() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		write_csv(
			&csv_path,
			"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n/path/one.dcm,2,\"[[1,2,3,4],[5,6,7,8]]\",\"[[0]]\"\n",
		);

		let error = load_annotations_for_files(&csv_path, &[]).expect_err("mismatched frame groups should fail");
		assert!(error
			.to_string()
			.contains("len(ROI_frames) must equal num_ROI when ROI_frames is not empty"));
	}

	#[test]
	fn errors_when_frame_index_exceeds_matched_file_frame_count() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		let matched_file = dir.path().join("matched.dcm");
		write_csv(
			&csv_path,
			&format!(
				"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n{matched},1,\"[[1,2,3,4]]\",\"[[3]]\"\n",
				matched = matched_file.display(),
			),
		);

		let files = vec![file_entry(0, matched_file, 3)];
		let error = load_annotations_for_files(&csv_path, &files).expect_err("out-of-range frame should fail");
		assert!(error.to_string().contains("contains frame 3"));
		assert!(error.to_string().contains("3 frame(s)"));
	}

	#[test]
	fn errors_on_duplicate_anon_dicom_paths() {
		let dir = tempdir().expect("temp dir");
		let csv_path = dir.path().join("annotations.csv");
		let duplicate_path = dir.path().join("dup.dcm");
		write_csv(
			&csv_path,
			&format!(
				"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n{path},1,\"[[1,2,3,4]]\",\"[]\"\n{path},1,\"[[5,6,7,8]]\",\"[]\"\n",
				path = duplicate_path.display(),
			),
		);

		let error = load_annotations_for_files(&csv_path, &[]).expect_err("duplicate path should fail");
		assert!(error.to_string().contains("duplicate anon_dicom_path"));
	}

	fn file_entry(index: usize, path: PathBuf, frame_count: u32) -> FileEntry {
		FileEntry {
			index,
			path,
			label: "fixture".to_string(),
			has_pixels: true,
			frame_count,
			rows: 1,
			columns: 1,
			transfer_syntax_uid: "1.2.840.10008.1.2.1".to_string(),
			default_window: None,
		}
	}

	fn write_csv(path: &Path, content: &str) {
		fs::write(path, content).expect("write csv");
	}
}
