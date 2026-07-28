pub use crate::api::contracts::EmbedRoiAnnotations;
use crate::types::FileEntry;
use anyhow::{anyhow, bail, Context, Result};
use csv::{ReaderBuilder, StringRecord};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct ParsedAnnotationRow {
    row_number: usize,
    normalized_path: String,
    annotations: EmbedRoiAnnotations,
}

pub type AnnotationIndexMap = HashMap<usize, EmbedRoiAnnotations>;

#[derive(Debug, Clone)]
pub struct AnnotationStore {
    inner: Arc<Mutex<AnnotationStoreInner>>,
}

#[derive(Debug, Clone)]
struct AnnotationStoreInner {
    annotations: AnnotationIndexMap,
    user_edited: HashSet<usize>,
}

#[derive(Debug, Clone)]
pub struct AnnotationSource {
    rows: Vec<ParsedAnnotationRow>,
}

impl AnnotationStore {
    pub fn new(annotations: AnnotationIndexMap) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AnnotationStoreInner {
                annotations,
                user_edited: HashSet::new(),
            })),
        }
    }

    pub fn empty() -> Self {
        Self::new(HashMap::new())
    }

    pub fn get(&self, file_index: usize) -> Result<EmbedRoiAnnotations> {
        let annotations = self
            .inner
            .lock()
            .map_err(|_| anyhow!("annotations store lock poisoned"))?;
        Ok(annotations
            .annotations
            .get(&file_index)
            .cloned()
            .unwrap_or_else(EmbedRoiAnnotations::empty))
    }

    pub fn replace_all(&self, annotations: AnnotationIndexMap) -> Result<()> {
        let mut store = self
            .inner
            .lock()
            .map_err(|_| anyhow!("annotations store lock poisoned"))?;
        store.annotations = annotations;
        store.user_edited.clear();
        Ok(())
    }

    pub fn insert_csv_if_unedited(
        &self,
        file_index: usize,
        annotations: EmbedRoiAnnotations,
    ) -> Result<()> {
        let mut store = self
            .inner
            .lock()
            .map_err(|_| anyhow!("annotations store lock poisoned"))?;
        if !store.user_edited.contains(&file_index) {
            store.annotations.insert(file_index, annotations);
        }
        Ok(())
    }

    pub fn replace_for_file(
        &self,
        file: &FileEntry,
        annotations: EmbedRoiAnnotations,
    ) -> Result<EmbedRoiAnnotations> {
        let canonical =
            canonicalize_annotations(annotations, file.rows, file.columns, file.frame_count)?;
        let mut store = self
            .inner
            .lock()
            .map_err(|_| anyhow!("annotations store lock poisoned"))?;
        store.user_edited.insert(file.index);
        if canonical.num_roi == 0 {
            store.annotations.remove(&file.index);
        } else {
            store.annotations.insert(file.index, canonical.clone());
        }
        Ok(canonical)
    }

    pub fn export_embed_csv(&self, files: &[FileEntry]) -> Result<String> {
        let store = self
            .inner
            .lock()
            .map_err(|_| anyhow!("annotations store lock poisoned"))?;
        let mut writer = csv::Writer::from_writer(Vec::new());
        writer.write_record(["anon_dicom_path", "num_ROI", "ROI_coords", "ROI_frames"])?;

        for file in files {
            let Some(annotations) = store.annotations.get(&file.index) else {
                continue;
            };
            if annotations.num_roi == 0 {
                continue;
            }
            writer.write_record([
                file.path.to_string_lossy().into_owned(),
                annotations.num_roi.to_string(),
                serde_json::to_string(&annotations.roi_coords)?,
                serde_json::to_string(&annotations.roi_frames)?,
            ])?;
        }

        let bytes = writer.into_inner().map_err(|error| error.into_error())?;
        String::from_utf8(bytes).context("annotations CSV export was not valid UTF-8")
    }
}

struct ColumnIndexes {
    path: usize,
    roi_coords: usize,
    num_roi: Option<usize>,
    roi_frames: Option<usize>,
}

impl AnnotationSource {
    pub fn from_path(csv_path: &Path) -> Result<Self> {
        Ok(Self {
            rows: parse_rows(csv_path)?,
        })
    }

    pub fn annotations_for_file(&self, file: &FileEntry) -> Result<Option<EmbedRoiAnnotations>> {
        let normalized = normalize_path(&file.path);
        for row in &self.rows {
            if row.normalized_path == normalized {
                validate_frames_in_range(&row.annotations, file.frame_count, row.row_number)?;
                return Ok(Some(row.annotations.clone()));
            }
        }
        Ok(None)
    }

    pub fn load_for_files(&self, files: &[FileEntry]) -> Result<AnnotationIndexMap> {
        let file_lookup = build_file_lookup(files)?;

        let mut annotations_by_file = HashMap::new();
        for row in &self.rows {
            if let Some(file_targets) = file_lookup.get(&row.normalized_path) {
                for &(file_index, frame_count) in file_targets {
                    validate_frames_in_range(&row.annotations, frame_count, row.row_number)?;
                    annotations_by_file.insert(file_index, row.annotations.clone());
                }
            }
        }

        Ok(annotations_by_file)
    }

    pub fn unmatched_row_count(&self, files: &[FileEntry]) -> usize {
        let matched_paths: HashSet<String> = files
            .iter()
            .map(|file| normalize_path(&file.path))
            .collect();
        self.rows
            .iter()
            .filter(|row| !matched_paths.contains(&row.normalized_path))
            .count()
    }
}

pub fn load_annotations_for_files(
    csv_path: &Path,
    files: &[FileEntry],
) -> Result<AnnotationIndexMap> {
    AnnotationSource::from_path(csv_path)?.load_for_files(files)
}

fn parse_rows(csv_path: &Path) -> Result<Vec<ParsedAnnotationRow>> {
    let mut reader = ReaderBuilder::new()
        .flexible(false)
        .from_path(csv_path)
        .with_context(|| format!("failed to open annotations CSV: {}", csv_path.display()))?;

    let headers = reader
        .headers()
        .with_context(|| {
            format!(
                "failed to read annotations CSV header: {}",
                csv_path.display()
            )
        })?
        .clone();
    let indexes = build_column_indexes(&headers)?;

    let mut rows = Vec::new();
    let mut seen_paths = HashMap::<String, usize>::new();

    for (idx, row_result) in reader.records().enumerate() {
        let row_number = idx + 2; // Header is line 1.
        let row = row_result
            .with_context(|| format!("annotations CSV row {row_number} could not be parsed"))?;
        let parsed = parse_row(&row, &indexes, row_number)?;

        if let Some(previous_row) =
            seen_paths.insert(parsed.normalized_path.clone(), parsed.row_number)
        {
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

fn build_column_indexes(headers: &StringRecord) -> Result<ColumnIndexes> {
    let find = |name: &str| headers.iter().position(|h| h == name);

    let path = find("anon_dicom_path")
        .ok_or_else(|| anyhow!("annotations CSV missing required column `anon_dicom_path`"))?;
    let roi_coords = find("ROI_coords")
        .ok_or_else(|| anyhow!("annotations CSV missing required column `ROI_coords`"))?;

    Ok(ColumnIndexes {
        path,
        roi_coords,
        num_roi: find("num_ROI"),
        roi_frames: find("ROI_frames"),
    })
}

fn parse_row(
    row: &StringRecord,
    indexes: &ColumnIndexes,
    row_number: usize,
) -> Result<ParsedAnnotationRow> {
    let raw_path = row.get(indexes.path).ok_or_else(|| {
        anyhow!("annotations CSV row {row_number}: missing value for `anon_dicom_path`")
    })?;
    let normalized_path = normalize_path(Path::new(raw_path.trim()));
    if normalized_path.is_empty() {
        bail!("annotations CSV row {row_number}: anon_dicom_path must not be empty");
    }

    let raw_roi_coords = row.get(indexes.roi_coords).ok_or_else(|| {
        anyhow!("annotations CSV row {row_number}: missing value for `ROI_coords`")
    })?;
    let roi_coords = parse_roi_coords(raw_roi_coords.trim(), row_number)?;

    let num_roi = if let Some(num_roi_idx) = indexes.num_roi {
        let raw = row.get(num_roi_idx).ok_or_else(|| {
            anyhow!("annotations CSV row {row_number}: missing value for `num_ROI`")
        })?;
        let parsed = raw.trim().parse::<usize>().map_err(|error| {
            anyhow!("annotations CSV row {row_number}: num_ROI must be an integer: {error}")
        })?;
        if parsed != roi_coords.len() {
            bail!(
                "annotations CSV row {row_number}: len(ROI_coords) must equal num_ROI ({} != {})",
                roi_coords.len(),
                parsed
            );
        }
        parsed
    } else {
        roi_coords.len()
    };

    let roi_frames = if let Some(roi_frames_idx) = indexes.roi_frames {
        let raw = row.get(roi_frames_idx).ok_or_else(|| {
            anyhow!("annotations CSV row {row_number}: missing value for `ROI_frames`")
        })?;
        let frames = parse_roi_frames(raw.trim(), row_number)?;
        if !frames.is_empty() && frames.len() != num_roi {
            bail!(
				"annotations CSV row {row_number}: len(ROI_frames) must equal num_ROI when ROI_frames is not empty ({} != {})",
				frames.len(),
				num_roi
			);
        }
        frames
    } else {
        vec![]
    };

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
        lookup
            .entry(normalized)
            .or_default()
            .push((file.index, file.frame_count));
    }
    Ok(lookup)
}

fn validate_frames_in_range(
    annotations: &EmbedRoiAnnotations,
    frame_count: u32,
    row_number: usize,
) -> Result<()> {
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

pub fn canonicalize_annotations(
    annotations: EmbedRoiAnnotations,
    rows: u32,
    columns: u32,
    frame_count: u32,
) -> Result<EmbedRoiAnnotations> {
    if rows == 0 || columns == 0 {
        bail!("annotations cannot be edited for files without image dimensions");
    }

    let mut roi_coords = Vec::with_capacity(annotations.roi_coords.len());
    for (idx, [ymin, xmin, ymax, xmax]) in annotations.roi_coords.into_iter().enumerate() {
        let y0 = ymin.min(ymax);
        let y1 = ymin.max(ymax);
        let x0 = xmin.min(xmax);
        let x1 = xmin.max(xmax);

        if y0 == y1 || x0 == x1 {
            bail!("ROI_coords[{idx}] must describe a non-empty rectangle");
        }
        if y1 > rows || x1 > columns {
            bail!(
				"ROI_coords[{idx}] exceeds image bounds: [{y0}, {x0}, {y1}, {x1}] outside {rows}x{columns}"
			);
        }

        roi_coords.push([y0, x0, y1, x1]);
    }

    let num_roi = roi_coords.len();
    let roi_frames = if annotations.roi_frames.is_empty() {
        Vec::new()
    } else {
        if annotations.roi_frames.len() != num_roi {
            bail!(
                "len(ROI_frames) must equal ROI count when ROI_frames is not empty ({} != {})",
                annotations.roi_frames.len(),
                num_roi
            );
        }
        for (roi_idx, frames) in annotations.roi_frames.iter().enumerate() {
            for frame in frames {
                if *frame >= frame_count {
                    bail!(
                        "ROI_frames[{roi_idx}] contains frame {frame}, but DICOM has {} frame(s)",
                        frame_count
                    );
                }
            }
        }
        annotations.roi_frames
    };

    Ok(EmbedRoiAnnotations {
        num_roi,
        roi_coords,
        roi_frames,
    })
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
    use super::{
        canonicalize_annotations, load_annotations_for_files, AnnotationSource, AnnotationStore,
        EmbedRoiAnnotations,
    };
    use crate::types::FileEntry;
    use std::collections::HashMap;
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
        let mapped =
            load_annotations_for_files(&csv_path, &files).expect("annotations should parse");

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
        let mapped =
            load_annotations_for_files(&csv_path, &files).expect("annotations should parse");
        assert_eq!(
            mapped.get(&0).map(|value| value.roi_frames.clone()),
            Some(vec![])
        );
    }

    #[test]
    fn accepts_csv_without_num_roi_column() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        let matched_file = dir.path().join("matched.dcm");

        write_csv(
			&csv_path,
			&format!(
				"anon_dicom_path,ROI_coords,ROI_frames\n{matched},\"[[10,20,30,40],[50,60,70,80]]\",\"[[0],[1]]\"\n",
				matched = matched_file.display(),
			),
		);

        let files = vec![file_entry(0, matched_file.clone(), 3)];
        let mapped = load_annotations_for_files(&csv_path, &files)
            .expect("annotations should parse without num_ROI");
        assert_eq!(mapped.get(&0).map(|a| a.num_roi), Some(2));
    }

    #[test]
    fn accepts_csv_without_roi_frames_column() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        let matched_file = dir.path().join("matched.dcm");

        write_csv(
            &csv_path,
            &format!(
                "anon_dicom_path,ROI_coords\n{matched},\"[[1,2,3,4]]\"\n",
                matched = matched_file.display(),
            ),
        );

        let files = vec![file_entry(0, matched_file.clone(), 10)];
        let mapped = load_annotations_for_files(&csv_path, &files)
            .expect("annotations should parse without ROI_frames");
        assert_eq!(mapped.get(&0).map(|a| a.roi_frames.clone()), Some(vec![]));
    }

    #[test]
    fn parsed_source_matches_single_files_and_counts_unmatched_rows() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        let matched_file = dir.path().join("matched.dcm");
        let unmatched_file = dir.path().join("unmatched.dcm");

        write_csv(
            &csv_path,
            &format!(
                "anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n{matched},1,\"[[1,2,3,4]]\",\"[[0]]\"\n{unmatched},1,\"[[5,6,7,8]]\",\"[[0]]\"\n",
                matched = matched_file.display(),
                unmatched = unmatched_file.display(),
            ),
        );

        let source = AnnotationSource::from_path(&csv_path).expect("source parses");
        let matched = file_entry(0, matched_file.clone(), 1);

        let annotations = source
            .annotations_for_file(&matched)
            .expect("single-file match succeeds")
            .expect("matched annotations");

        assert_eq!(annotations.roi_coords, vec![[1, 2, 3, 4]]);
        assert_eq!(source.unmatched_row_count(&[matched]), 1);
    }

    #[test]
    fn errors_when_required_column_is_missing() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        write_csv(
            &csv_path,
            "anon_dicom_path,num_ROI,ROI_frames\n/path/one.dcm,1,\"[]\"\n",
        );

        let error =
            load_annotations_for_files(&csv_path, &[]).expect_err("missing header should fail");
        assert!(error
            .to_string()
            .contains("missing required column `ROI_coords`"));
    }

    #[test]
    fn errors_when_num_roi_and_coords_count_do_not_align() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        write_csv(
			&csv_path,
			"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n/path/one.dcm,2,\"[[1,2,3,4]]\",\"[]\"\n",
		);

        let error = load_annotations_for_files(&csv_path, &[])
            .expect_err("mismatched ROI count should fail");
        assert!(error
            .to_string()
            .contains("len(ROI_coords) must equal num_ROI"));
    }

    #[test]
    fn errors_when_roi_frames_length_does_not_match_num_roi() {
        let dir = tempdir().expect("temp dir");
        let csv_path = dir.path().join("annotations.csv");
        write_csv(
			&csv_path,
			"anon_dicom_path,num_ROI,ROI_coords,ROI_frames\n/path/one.dcm,2,\"[[1,2,3,4],[5,6,7,8]]\",\"[[0]]\"\n",
		);

        let error = load_annotations_for_files(&csv_path, &[])
            .expect_err("mismatched frame groups should fail");
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
        let error = load_annotations_for_files(&csv_path, &files)
            .expect_err("out-of-range frame should fail");
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

        let error =
            load_annotations_for_files(&csv_path, &[]).expect_err("duplicate path should fail");
        assert!(error.to_string().contains("duplicate anon_dicom_path"));
    }

    #[test]
    fn canonicalizes_coords_and_derives_roi_count() {
        let annotations = EmbedRoiAnnotations {
            num_roi: 99,
            roi_coords: vec![[30, 40, 10, 20]],
            roi_frames: vec![vec![0, 1]],
        };

        let canonical =
            canonicalize_annotations(annotations, 100, 100, 2).expect("canonical annotations");

        assert_eq!(
            canonical,
            EmbedRoiAnnotations {
                num_roi: 1,
                roi_coords: vec![[10, 20, 30, 40]],
                roi_frames: vec![vec![0, 1]],
            }
        );
    }

    #[test]
    fn rejects_edit_coords_outside_image_bounds() {
        let annotations = EmbedRoiAnnotations {
            num_roi: 1,
            roi_coords: vec![[0, 0, 2, 1]],
            roi_frames: vec![],
        };

        let error = canonicalize_annotations(annotations, 1, 1, 1).expect_err("bounds should fail");

        assert!(error.to_string().contains("exceeds image bounds"));
    }

    #[test]
    fn exports_embed_csv_with_json_quoted_columns() {
        let file = file_entry(0, PathBuf::from("/tmp/exported.dcm"), 3);
        let store = AnnotationStore::new(HashMap::from([(
            0,
            EmbedRoiAnnotations {
                num_roi: 1,
                roi_coords: vec![[1, 2, 3, 4]],
                roi_frames: vec![vec![1, 2]],
            },
        )]));

        let csv = store.export_embed_csv(&[file]).expect("export csv");

        assert!(csv.contains("anon_dicom_path,num_ROI,ROI_coords,ROI_frames"));
        assert!(csv.contains("/tmp/exported.dcm,1,\"[[1,2,3,4]]\",\"[[1,2]]\""));
    }

    #[test]
    fn csv_insert_does_not_overwrite_user_edit() {
        let mut file = file_entry(0, PathBuf::from("/tmp/edited.dcm"), 3);
        file.rows = 100;
        file.columns = 100;
        let store = AnnotationStore::empty();

        let edited = store
            .replace_for_file(
                &file,
                EmbedRoiAnnotations {
                    num_roi: 1,
                    roi_coords: vec![[1, 2, 3, 4]],
                    roi_frames: vec![vec![0]],
                },
            )
            .expect("edit annotations");

        store
            .insert_csv_if_unedited(
                file.index,
                EmbedRoiAnnotations {
                    num_roi: 1,
                    roi_coords: vec![[10, 20, 30, 40]],
                    roi_frames: vec![vec![1]],
                },
            )
            .expect("csv insert");

        assert_eq!(store.get(file.index).expect("read annotations"), edited);
    }

    fn file_entry(index: usize, path: PathBuf, frame_count: u32) -> FileEntry {
        FileEntry {
            index,
            path,
            label: "fixture".to_string(),
            patient_id: String::new(),
            patient_name: String::new(),
            study_instance_uid: String::new(),
            study_date: String::new(),
            study_description: String::new(),
            series_instance_uid: String::new(),
            series_number: String::new(),
            series_description: String::new(),
            modality: String::new(),
            instance_number: String::new(),
            sop_instance_uid: String::new(),
            has_pixels: true,
            frame_count,
            rows: 1,
            columns: 1,
            bits_allocated: 8,
            pixel_representation: 0,
            samples_per_pixel: 1,
            photometric_interpretation: "MONOCHROME2".to_string(),
            rescale_slope: 1.0,
            rescale_intercept: 0.0,
            transfer_syntax_uid: "1.2.840.10008.1.2.1".to_string(),
            default_window: None,
        }
    }

    fn write_csv(path: &Path, content: &str) {
        fs::write(path, content).expect("write csv");
    }
}
