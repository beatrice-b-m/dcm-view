use crate::types::{FileEntry, LoadReport, WindowPreset};
use anyhow::{anyhow, Context, Result};
use dicom_dictionary_std::{tags, uids};
use dicom_object::OpenFileOptions;
use rayon::prelude::*;
use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;
use tokio::task;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct DiscoverOptions {
    pub recursive: bool,
    pub filters: Vec<ScanFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanFilterField {
    PatientId,
    PatientName,
    StudyDescription,
    StudyDate,
    StudyUid,
    SeriesDescription,
    SeriesNumber,
    SeriesUid,
    Modality,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFilter {
    pub field: ScanFilterField,
    pub value: String,
}

impl ScanFilter {
    pub const VALID_FIELDS: &'static [&'static str] = &[
        "patient_id",
        "patient_name",
        "study_description",
        "study_date",
        "study_uid",
        "series_description",
        "series_number",
        "series_uid",
        "modality",
    ];

    pub fn matches(&self, entry: &FileEntry) -> bool {
        let haystack = match self.field {
            ScanFilterField::PatientId => &entry.patient_id,
            ScanFilterField::PatientName => &entry.patient_name,
            ScanFilterField::StudyDescription => &entry.study_description,
            ScanFilterField::StudyDate => &entry.study_date,
            ScanFilterField::StudyUid => &entry.study_instance_uid,
            ScanFilterField::SeriesDescription => &entry.series_description,
            ScanFilterField::SeriesNumber => &entry.series_number,
            ScanFilterField::SeriesUid => &entry.series_instance_uid,
            ScanFilterField::Modality => &entry.modality,
        };
        haystack.to_lowercase().contains(&self.value.to_lowercase())
    }
}

impl std::fmt::Display for ScanFilter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}={}", self.field, self.value)
    }
}

impl std::fmt::Display for ScanFilterField {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let field = match self {
            ScanFilterField::PatientId => "patient_id",
            ScanFilterField::PatientName => "patient_name",
            ScanFilterField::StudyDescription => "study_description",
            ScanFilterField::StudyDate => "study_date",
            ScanFilterField::StudyUid => "study_uid",
            ScanFilterField::SeriesDescription => "series_description",
            ScanFilterField::SeriesNumber => "series_number",
            ScanFilterField::SeriesUid => "series_uid",
            ScanFilterField::Modality => "modality",
        };
        formatter.write_str(field)
    }
}

impl FromStr for ScanFilter {
    type Err = String;

    fn from_str(raw: &str) -> std::result::Result<Self, Self::Err> {
        let (field, value) = raw
            .split_once('=')
            .ok_or_else(|| scan_filter_parse_error(raw))?;
        let field_name = field.trim().to_ascii_lowercase();
        let field = match field_name.as_str() {
            "patient_id" => ScanFilterField::PatientId,
            "patient_name" => ScanFilterField::PatientName,
            "study_description" => ScanFilterField::StudyDescription,
            "study_date" => ScanFilterField::StudyDate,
            "study_uid" => ScanFilterField::StudyUid,
            "series_description" => ScanFilterField::SeriesDescription,
            "series_number" => ScanFilterField::SeriesNumber,
            "series_uid" => ScanFilterField::SeriesUid,
            "modality" => ScanFilterField::Modality,
            _ => return Err(scan_filter_parse_error(raw)),
        };
        let value = value.trim();
        if value.is_empty() {
            return Err(scan_filter_parse_error(raw));
        }
        Ok(Self {
            field,
            value: value.to_string(),
        })
    }
}

fn scan_filter_parse_error(raw: &str) -> String {
    format!(
        "invalid scan filter `{raw}`; expected FIELD=VALUE where FIELD is one of: {}",
        ScanFilter::VALID_FIELDS.join(", ")
    )
}

#[derive(Debug)]
pub enum DiscoveryEvent {
    File(Box<FileEntry>),
    Skipped,
    Filtered,
}

#[derive(Debug, Clone)]
pub struct DiscoveryReport {
    pub files_found: usize,
    pub skipped: usize,
    pub filtered: usize,
    pub searched_recursive: bool,
}

pub async fn discover(paths: &[PathBuf], options: DiscoverOptions) -> Result<LoadReport> {
    let paths = paths.to_vec();
    task::spawn_blocking(move || discover_blocking(&paths, &options))
        .await
        .context("loader worker panicked")?
}

pub async fn discover_progressive(
    paths: &[PathBuf],
    options: DiscoverOptions,
    events: mpsc::Sender<DiscoveryEvent>,
) -> Result<DiscoveryReport> {
    let paths = paths.to_vec();
    task::spawn_blocking(move || discover_progressive_blocking(&paths, &options, events))
        .await
        .context("loader worker panicked")?
}

fn collect_candidates(paths: &[PathBuf], options: &DiscoverOptions) -> (Vec<PathBuf>, usize) {
    let mut candidates = Vec::new();
    let mut skipped = 0_usize;

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

            for entry in walker.into_iter() {
                match entry {
                    Ok(dir_entry) if dir_entry.path().is_file() => {
                        candidates.push(dir_entry.path().to_path_buf());
                    }
                    Ok(_) => {}
                    Err(error) => {
                        skipped += 1;
                        eprintln!("dcmview: warning — could not read path entry: {error}");
                    }
                }
            }
            continue;
        }

        skipped += 1;
        eprintln!(
            "dcmview: warning — input path does not exist or is unsupported: {}",
            path.display()
        );
    }

    (candidates, skipped)
}

fn discover_blocking(paths: &[PathBuf], options: &DiscoverOptions) -> Result<LoadReport> {
    let (candidates, mut skipped) = collect_candidates(paths, options);

    let processed: Vec<_> = candidates
        .par_iter()
        .map(|candidate| build_entry(candidate))
        .collect();

    let mut files = Vec::new();
    let mut filtered = 0_usize;

    for item in processed {
        match item {
            Ok(Some(entry)) if matches_filters(&entry, &options.filters) => files.push(entry),
            Ok(Some(_)) => filtered += 1,
            Ok(None) => skipped += 1,
            Err(error) => {
                skipped += 1;
                eprintln!("dcmview: warning — failed to inspect DICOM: {error}");
            }
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
        filtered,
        searched_recursive: options.recursive,
    })
}

fn discover_progressive_blocking(
    paths: &[PathBuf],
    options: &DiscoverOptions,
    events: mpsc::Sender<DiscoveryEvent>,
) -> Result<DiscoveryReport> {
    let (candidates, initial_skipped) = collect_candidates(paths, options);
    for _ in 0..initial_skipped {
        let _ = events.blocking_send(DiscoveryEvent::Skipped);
    }

    let files_found = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(initial_skipped);
    let filtered = AtomicUsize::new(0);

    candidates
        .par_iter()
        .for_each_with(events, |events, candidate| match build_entry(candidate) {
            Ok(Some(entry)) if matches_filters(&entry, &options.filters) => {
                files_found.fetch_add(1, Ordering::Relaxed);
                let _ = events.blocking_send(DiscoveryEvent::File(Box::new(entry)));
            }
            Ok(Some(_)) => {
                filtered.fetch_add(1, Ordering::Relaxed);
                let _ = events.blocking_send(DiscoveryEvent::Filtered);
            }
            Ok(None) => {
                skipped.fetch_add(1, Ordering::Relaxed);
                let _ = events.blocking_send(DiscoveryEvent::Skipped);
            }
            Err(error) => {
                skipped.fetch_add(1, Ordering::Relaxed);
                let _ = events.blocking_send(DiscoveryEvent::Skipped);
                eprintln!("dcmview: warning — failed to inspect DICOM: {error}");
            }
        });

    Ok(DiscoveryReport {
        files_found: files_found.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        filtered: filtered.load(Ordering::Relaxed),
        searched_recursive: options.recursive,
    })
}

fn matches_filters(entry: &FileEntry, filters: &[ScanFilter]) -> bool {
    filters.iter().all(|filter| filter.matches(entry))
}

fn build_entry(path: &Path) -> Result<Option<FileEntry>> {
    if !has_dicm_preamble(path)? {
        return Ok(None);
    }

    let obj = match OpenFileOptions::new()
        .read_until(tags::PIXEL_DATA)
        .open_file(path)
    {
        Ok(obj) => obj,
        Err(_) => return Ok(None),
    };

    let transfer_syntax_uid = obj.meta().transfer_syntax().to_string();
    let patient_id = read_str(&obj, "PatientID").unwrap_or_default();
    let patient_name = read_str(&obj, "PatientName").unwrap_or_default();
    let modality = read_str(&obj, "Modality").unwrap_or_default();
    let sop_instance_uid = read_str(&obj, "SOPInstanceUID").unwrap_or_default();
    let study_instance_uid = read_str(&obj, "StudyInstanceUID").unwrap_or_default();
    let study_date = read_str(&obj, "StudyDate").unwrap_or_default();
    let study_description = read_str(&obj, "StudyDescription").unwrap_or_default();
    let series_instance_uid = read_str(&obj, "SeriesInstanceUID").unwrap_or_default();
    let series_number = read_str(&obj, "SeriesNumber").unwrap_or_default();
    let series_description = read_str(&obj, "SeriesDescription").unwrap_or_default();
    let instance_number = read_str(&obj, "InstanceNumber").unwrap_or_default();
    let frame_count = read_u32(&obj, "NumberOfFrames").unwrap_or(1);
    let rows = read_u32(&obj, "Rows").unwrap_or(0);
    let columns = read_u32(&obj, "Columns").unwrap_or(0);
    let bits_allocated = read_u32(&obj, "BitsAllocated").unwrap_or(8);
    let pixel_representation = read_u32(&obj, "PixelRepresentation").unwrap_or(0);
    let samples_per_pixel = read_u32(&obj, "SamplesPerPixel").unwrap_or(1).max(1);
    let photometric_interpretation =
        read_str(&obj, "PhotometricInterpretation").unwrap_or_else(|| "MONOCHROME2".to_string());
    let rescale_slope = read_f64(&obj, "RescaleSlope").unwrap_or(1.0);
    let rescale_intercept = read_f64(&obj, "RescaleIntercept").unwrap_or(0.0);
    let has_pixels = has_pixel_data_tag(path, &transfer_syntax_uid)?;
    let default_window = match (
        read_f64(&obj, "WindowCenter"),
        read_f64(&obj, "WindowWidth"),
    ) {
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
        patient_id,
        patient_name,
        study_instance_uid,
        study_date,
        study_description,
        series_instance_uid,
        series_number,
        series_description,
        modality,
        instance_number,
        sop_instance_uid,
        has_pixels,
        frame_count,
        rows,
        columns,
        bits_allocated,
        pixel_representation,
        samples_per_pixel,
        photometric_interpretation,
        rescale_slope,
        rescale_intercept,
        transfer_syntax_uid,
        default_window,
    }))
}

fn has_dicm_preamble(path: &Path) -> Result<bool> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut preamble = [0_u8; 132];
    match file.read_exact(&mut preamble) {
        Ok(()) => Ok(&preamble[128..132] == b"DICM"),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn has_pixel_data_tag(path: &Path, transfer_syntax_uid: &str) -> Result<bool> {
    if transfer_syntax_uid == uids::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN {
        let obj = OpenFileOptions::new()
            .read_all()
            .open_file(path)
            .with_context(|| format!("failed to inspect deflated DICOM {}", path.display()))?;
        return Ok(obj.element(tags::PIXEL_DATA).is_ok());
    }

    let needle: &[u8] = if transfer_syntax_uid == "1.2.840.10008.1.2.2" {
        &[0x7f, 0xe0, 0x00, 0x10]
    } else {
        &[0xe0, 0x7f, 0x10, 0x00]
    };
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    file.seek(io::SeekFrom::Start(132))
        .with_context(|| format!("failed to seek {}", path.display()))?;
    let mut carried = Vec::<u8>::new();
    let mut chunk = [0_u8; 8192];

    loop {
        let read = file
            .read(&mut chunk)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            return Ok(false);
        }

        let carry_len = carried.len();
        carried.extend_from_slice(&chunk[..read]);
        if carried.windows(needle.len()).any(|window| window == needle) {
            return Ok(true);
        }

        let keep = needle.len().saturating_sub(1).min(carried.len());
        carried.drain(..carried.len().saturating_sub(keep));
        debug_assert!(carried.len() <= carry_len.max(needle.len().saturating_sub(1)));
    }
}

fn read_str(obj: &dicom_object::DefaultDicomObject, name: &str) -> Option<String> {
    obj.element_by_name(name)
        .ok()
        .and_then(|element| element.to_str().ok())
        .map(|value| {
            value
                .split('\\')
                .next()
                .unwrap_or(value.as_ref())
                .trim()
                .to_string()
        })
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
