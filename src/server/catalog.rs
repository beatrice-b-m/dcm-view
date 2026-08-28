use crate::api::contracts::{
    FileSummary, FrameRefSummary, SeriesCatalogResponse, SeriesStackSummary, SeriesSummary,
    SeriesWarningSummary,
};
use crate::loader::{DiscoveryDisposition, DiscoveryRecord};
use crate::series::{
    FrameOrderingInput, NavigationInput, NavigationKind, OrderingInput, SeriesCatalog,
    SeriesFileInput, SeriesGroup, SeriesStack, SeriesWarning,
};
use crate::types::FileEntry;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::{futures::Notified, Notify};

pub const DISCOVERY_RESPONSE_MAX_RECORDS: usize = 256;

#[derive(Clone)]
pub struct FileRegistry {
    inner: Arc<RwLock<FileRegistryInner>>,
    scanned: Arc<AtomicUsize>,
    skipped: Arc<AtomicUsize>,
    filtered: Arc<AtomicUsize>,
    scan_complete: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct FileRegistryInner {
    files: Vec<FileEntry>,
    summaries: Vec<FileSummary>,
    discovery_ledger: Vec<DiscoveryRecord>,
}

#[derive(Debug, Clone, Copy)]
pub struct RegistryStatus {
    pub file_count: usize,
    pub scanned: usize,
    pub skipped: usize,
    pub filtered: usize,
    pub scan_complete: bool,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FileRegistryInner::default())),
            scanned: Arc::new(AtomicUsize::new(0)),
            skipped: Arc::new(AtomicUsize::new(0)),
            filtered: Arc::new(AtomicUsize::new(0)),
            scan_complete: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn from_files(files: Vec<FileEntry>) -> Self {
        let registry = Self::new();
        for file in files {
            registry.insert(file);
            registry.record_scanned();
        }
        registry.mark_scan_complete();
        registry
    }

    pub fn insert(&self, mut file: FileEntry) -> usize {
        let mut inner = self.inner.write().expect("file registry lock poisoned");
        let index = inner.files.len();
        file.index = index;
        let summary = FileSummary::from(&file);
        inner.files.push(file);
        inner.summaries.push(summary);
        drop(inner);
        self.notify.notify_waiters();
        index
    }

    pub fn record_scanned(&self) {
        self.scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_filtered(&self) {
        self.filtered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_discovery(&self, record: DiscoveryRecord) {
        match record.disposition {
            DiscoveryDisposition::Selected => self.record_scanned(),
            DiscoveryDisposition::Skipped => self.record_skipped(),
            DiscoveryDisposition::Filtered => self.record_filtered(),
        }
        self.inner
            .write()
            .expect("file registry lock poisoned")
            .discovery_ledger
            .push(record);
        self.notify.notify_waiters();
    }

    pub fn mark_scan_complete(&self) {
        self.scan_complete.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub fn changed(&self) -> Notified<'_> {
        self.notify.notified()
    }

    pub fn get(&self, index: usize) -> Option<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .get(index)
            .cloned()
    }

    pub fn files_snapshot(&self) -> Vec<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .clone()
    }

    pub fn summaries_snapshot(&self) -> Vec<FileSummary> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .summaries
            .clone()
    }

    pub fn series_catalog_snapshot(&self) -> SeriesCatalogResponse {
        let files = self.files_snapshot();
        let catalog = SeriesCatalog::build(files.iter().map(series_file_input));
        SeriesCatalogResponse {
            series: catalog
                .series()
                .iter()
                .map(|group| series_summary(group, &files))
                .collect(),
            scan_complete: self.scan_complete.load(Ordering::Relaxed),
        }
    }

    pub fn discovery_ledger_snapshot(&self) -> Vec<DiscoveryRecord> {
        let mut records = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .discovery_ledger
            .clone();
        records.sort();
        records
    }

    pub fn discovery_response_snapshot(&self) -> Vec<DiscoveryRecord> {
        let mut records = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .discovery_ledger
            .iter()
            .rev()
            .take(DISCOVERY_RESPONSE_MAX_RECORDS)
            .cloned()
            .collect::<Vec<_>>();
        records.sort();
        records
    }

    pub fn status(&self) -> RegistryStatus {
        let file_count = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .len();
        RegistryStatus {
            file_count,
            scanned: self.scanned.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            filtered: self.filtered.load(Ordering::Relaxed),
            scan_complete: self.scan_complete.load(Ordering::Relaxed),
        }
    }
}

const VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";

fn series_file_input(file: &FileEntry) -> SeriesFileInput {
    let metadata = &file.series_metadata;
    let has_per_frame_geometry = !metadata.frame_image_positions_patient.is_empty()
        || !metadata.frame_image_orientations_patient.is_empty();
    let per_frame_ordering = if has_per_frame_geometry {
        (0..file.frame_count)
            .map(|frame_index| FrameOrderingInput {
                frame_index,
                ordering: OrderingInput {
                    image_position_patient: file.frame_image_position_patient(frame_index),
                    image_orientation_patient: file.frame_image_orientation_patient(frame_index),
                },
            })
            .collect()
    } else {
        Vec::new()
    };
    let navigation = if let Some(concatenation_uid) = metadata
        .concatenation_uid
        .as_ref()
        .filter(|uid| !uid.is_empty())
    {
        NavigationInput::Concatenation {
            concatenation_uid: concatenation_uid.clone(),
            concatenation_frame_offset_number: metadata.concatenation_frame_offset_number,
            in_concatenation_number: metadata.in_concatenation_number,
        }
    } else if file.sop_class_uid == VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE {
        NavigationInput::Wsi {
            pyramid_uid: metadata.pyramid_uid.clone().filter(|uid| !uid.is_empty()),
            image_type_role: metadata
                .image_type
                .get(2)
                .cloned()
                .filter(|role| !role.is_empty()),
            total_pixel_matrix_rows: metadata.total_pixel_matrix_rows,
            total_pixel_matrix_columns: metadata.total_pixel_matrix_columns,
        }
    } else {
        NavigationInput::Ordinary
    };

    SeriesFileInput {
        file_index: file.index,
        path: file.path.clone(),
        study_instance_uid: file.study_instance_uid.clone(),
        series_instance_uid: file.series_instance_uid.clone(),
        frame_of_reference_uid: metadata.frame_of_reference_uid.clone(),
        sop_instance_uid: file.sop_instance_uid.clone(),
        frame_count: file.frame_count,
        instance_number: file.instance_number.trim().parse().ok(),
        ordering: OrderingInput {
            image_position_patient: metadata.image_position_patient,
            image_orientation_patient: metadata.image_orientation_patient,
        },
        per_frame_ordering,
        navigation,
    }
}

fn series_summary(group: &SeriesGroup, files: &[FileEntry]) -> SeriesSummary {
    let frame_of_reference_uids = files
        .iter()
        .filter(|file| {
            file.study_instance_uid == group.id.study_instance_uid
                && file.series_instance_uid == group.id.series_instance_uid
        })
        .map(|file| file.series_metadata.frame_of_reference_uid.clone())
        .filter(|uid| !uid.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let id = series_id(&group.id.study_instance_uid, &group.id.series_instance_uid);
    SeriesSummary {
        id: id.clone(),
        study_instance_uid: group.id.study_instance_uid.clone(),
        series_instance_uid: group.id.series_instance_uid.clone(),
        frame_of_reference_uids,
        stacks: group
            .stacks
            .iter()
            .map(|stack| stack_summary(&id, stack))
            .collect(),
    }
}

fn series_id(study_instance_uid: &str, series_instance_uid: &str) -> String {
    format!("study:{study_instance_uid}|series:{series_instance_uid}")
}

fn stack_summary(series_id: &str, stack: &SeriesStack) -> SeriesStackSummary {
    let (kind, identity, concatenation_uid, pyramid_uid, image_type_role, rows, columns) =
        match &stack.kind {
            NavigationKind::Ordinary => (
                "ordinary",
                "ordinary".to_string(),
                None,
                None,
                None,
                None,
                None,
            ),
            NavigationKind::Concatenation { concatenation_uid } => (
                "concatenation",
                format!("concatenation:{concatenation_uid}"),
                Some(concatenation_uid.clone()),
                None,
                None,
                None,
                None,
            ),
            NavigationKind::WsiPyramidLevel {
                pyramid_uid,
                image_type_role,
                total_pixel_matrix_rows,
                total_pixel_matrix_columns,
            } => (
                "wsi_pyramid_level",
                format!(
                    "pyramid:{pyramid_uid}|role:{}|matrix:{}x{}",
                    image_type_role.as_deref().unwrap_or(""),
                    total_pixel_matrix_rows.map_or_else(String::new, |value| value.to_string()),
                    total_pixel_matrix_columns.map_or_else(String::new, |value| value.to_string())
                ),
                None,
                Some(pyramid_uid.clone()),
                image_type_role.clone(),
                *total_pixel_matrix_rows,
                *total_pixel_matrix_columns,
            ),
            NavigationKind::WsiCompanion {
                sop_instance_uid,
                image_type_role,
            } => (
                "wsi_companion",
                format!("companion:{sop_instance_uid}"),
                None,
                None,
                image_type_role.clone(),
                None,
                None,
            ),
        };
    SeriesStackSummary {
        id: format!("{series_id}|stack:{identity}"),
        kind: kind.to_string(),
        concatenation_uid,
        pyramid_uid,
        image_type_role,
        total_pixel_matrix_rows: rows,
        total_pixel_matrix_columns: columns,
        frames: stack
            .frames
            .iter()
            .map(|frame| FrameRefSummary {
                virtual_index: frame.virtual_index,
                file_index: frame.file_index,
                frame_index: frame.frame_index,
                source_path: frame.source_path.display().to_string(),
                sop_instance_uid: frame.sop_instance_uid.clone(),
                instance_number: frame
                    .instance_number
                    .and_then(|value| value.try_into().ok()),
                position_along_normal_mm: frame.position_along_normal_mm,
            })
            .collect(),
        warnings: stack.warnings.iter().map(warning_summary).collect(),
    }
}

fn warning_summary(warning: &SeriesWarning) -> SeriesWarningSummary {
    let (code, message, file_indices) = match warning {
        SeriesWarning::MissingPositions { frames } => (
            "missing_positions",
            format!(
                "{} frame source(s) have no Image Position Patient",
                frames.len()
            ),
            frame_file_indices(frames.iter().map(|frame| frame.file_index)),
        ),
        SeriesWarning::DuplicatePositions { groups } => (
            "duplicate_positions",
            format!("{} duplicate projected position group(s)", groups.len()),
            frame_file_indices(
                groups
                    .iter()
                    .flat_map(|group| group.frames.iter().map(|frame| frame.file_index)),
            ),
        ),
        SeriesWarning::NonuniformSpacing {
            adjacent_spacing_mm,
        } => (
            "nonuniform_spacing",
            format!("adjacent projected spacing is {adjacent_spacing_mm:?} mm"),
            Vec::new(),
        ),
        SeriesWarning::InconsistentOrientation { frames } => (
            "inconsistent_orientation",
            format!(
                "{} frame source(s) differ from the reference orientation",
                frames.len()
            ),
            frame_file_indices(frames.iter().map(|frame| frame.file_index)),
        ),
        SeriesWarning::GantryTilt {
            frames,
            max_lateral_shift_mm,
        } => (
            "gantry_tilt",
            format!("maximum in-plane position shift is {max_lateral_shift_mm:.6} mm"),
            frame_file_indices(frames.iter().map(|frame| frame.file_index)),
        ),
    };
    SeriesWarningSummary {
        code: code.to_string(),
        message,
        file_indices,
    }
}

fn frame_file_indices(indices: impl IntoIterator<Item = usize>) -> Vec<usize> {
    indices
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl Default for FileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileRegistry, DISCOVERY_RESPONSE_MAX_RECORDS};
    use crate::loader::{DiscoveryDisposition, DiscoveryReason, DiscoveryRecord};
    use std::path::PathBuf;

    #[test]
    fn discovery_ledger_is_memory_only_sorted_and_updates_counts() {
        let registry = FileRegistry::new();
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/z-invalid.bin"),
            disposition: DiscoveryDisposition::Skipped,
            reason: DiscoveryReason::MissingPart10Preamble,
        });
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/a-selected.dcm"),
            disposition: DiscoveryDisposition::Selected,
            reason: DiscoveryReason::ValidDicom,
        });
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/m-filtered.dcm"),
            disposition: DiscoveryDisposition::Filtered,
            reason: DiscoveryReason::FilterMismatch,
        });

        let records = registry.discovery_ledger_snapshot();
        assert_eq!(
            records
                .iter()
                .map(|record| record.path.as_path())
                .collect::<Vec<_>>(),
            vec![
                std::path::Path::new("/scan/a-selected.dcm"),
                std::path::Path::new("/scan/m-filtered.dcm"),
                std::path::Path::new("/scan/z-invalid.bin"),
            ]
        );
        assert_eq!(records[0].reason.code(), "valid_dicom");
        assert_eq!(records[1].reason.code(), "filter_mismatch");
        assert_eq!(records[2].reason.code(), "missing_part10_preamble");

        let status = registry.status();
        assert_eq!(status.scanned, 1);
        assert_eq!(status.skipped, 1);
        assert_eq!(status.filtered, 1);

        let clone = registry.clone();
        assert_eq!(clone.discovery_ledger_snapshot(), records);
        assert!(FileRegistry::new().discovery_ledger_snapshot().is_empty());
    }

    #[test]
    fn discovery_response_snapshot_is_bounded_to_recent_records() {
        let registry = FileRegistry::new();
        for index in 0..DISCOVERY_RESPONSE_MAX_RECORDS + 2 {
            registry.record_discovery(DiscoveryRecord {
                path: PathBuf::from(format!("/scan/{index:04}.dcm")),
                disposition: DiscoveryDisposition::Selected,
                reason: DiscoveryReason::ValidDicom,
            });
        }

        let response = registry.discovery_response_snapshot();
        assert_eq!(response.len(), DISCOVERY_RESPONSE_MAX_RECORDS);
        assert_eq!(response[0].path, PathBuf::from("/scan/0002.dcm"));
        assert_eq!(
            response.last().expect("last bounded record").path,
            PathBuf::from(format!(
                "/scan/{:04}.dcm",
                DISCOVERY_RESPONSE_MAX_RECORDS + 1
            ))
        );
        assert_eq!(
            registry.discovery_ledger_snapshot().len(),
            DISCOVERY_RESPONSE_MAX_RECORDS + 2
        );
    }
}
