use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowPreset {
    pub center: f64,
    pub width: f64,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub index: usize,
    pub path: PathBuf,
    pub label: String,
    pub patient_id: String,
    pub patient_name: String,
    pub study_instance_uid: String,
    pub study_date: String,
    pub study_description: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub series_description: String,
    pub modality: String,
    pub instance_number: String,
    pub sop_instance_uid: String,
    pub has_pixels: bool,
    pub frame_count: u32,
    pub rows: u32,
    pub columns: u32,
    pub bits_allocated: u32,
    pub pixel_representation: u32,
    pub samples_per_pixel: u32,
    pub photometric_interpretation: String,
    pub rescale_slope: f64,
    pub rescale_intercept: f64,
    pub transfer_syntax_uid: String,
    pub default_window: Option<WindowPreset>,
}

impl FileEntry {
    pub fn raw_metadata(
        &self,
        rows: u32,
        columns: u32,
        bits_allocated: u32,
        samples_per_pixel: u32,
    ) -> RawFrameMetadata {
        RawFrameMetadata {
            rows,
            columns,
            bits_allocated,
            pixel_representation: self.pixel_representation,
            samples_per_pixel,
            photometric_interpretation: self.photometric_interpretation.clone(),
            rescale_slope: self.rescale_slope,
            rescale_intercept: self.rescale_intercept,
            default_wc: self.default_window.map(|window| window.center),
            default_ww: self.default_window.map(|window| window.width),
        }
    }

    pub fn normalized_grayscale_u8_metadata(&self, rows: u32, columns: u32) -> RawFrameMetadata {
        RawFrameMetadata {
            rows,
            columns,
            bits_allocated: 8,
            pixel_representation: 0,
            samples_per_pixel: 1,
            photometric_interpretation: "MONOCHROME2".to_string(),
            rescale_slope: self.rescale_slope,
            rescale_intercept: self.rescale_intercept,
            default_wc: self.default_window.map(|window| window.center),
            default_ww: self.default_window.map(|window| window.width),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub index: usize,
    pub path: String,
    pub label: String,
    pub patient_id: String,
    pub patient_name: String,
    pub study_instance_uid: String,
    pub study_date: String,
    pub study_description: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub series_description: String,
    pub modality: String,
    pub instance_number: String,
    pub sop_instance_uid: String,
    pub has_pixels: bool,
    pub frame_count: u32,
    pub rows: u32,
    pub columns: u32,
    pub transfer_syntax_uid: String,
    pub default_window: Option<WindowPreset>,
}

impl From<&FileEntry> for FileSummary {
    fn from(value: &FileEntry) -> Self {
        Self {
            index: value.index,
            path: value.path.display().to_string(),
            label: value.label.clone(),
            patient_id: value.patient_id.clone(),
            patient_name: value.patient_name.clone(),
            study_instance_uid: value.study_instance_uid.clone(),
            study_date: value.study_date.clone(),
            study_description: value.study_description.clone(),
            series_instance_uid: value.series_instance_uid.clone(),
            series_number: value.series_number.clone(),
            series_description: value.series_description.clone(),
            modality: value.modality.clone(),
            instance_number: value.instance_number.clone(),
            sop_instance_uid: value.sop_instance_uid.clone(),
            has_pixels: value.has_pixels,
            frame_count: value.frame_count,
            rows: value.rows,
            columns: value.columns,
            transfer_syntax_uid: value.transfer_syntax_uid.clone(),
            default_window: value.default_window,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FilesResponse {
    pub files: Vec<FileSummary>,
    pub tunnelled: bool,
    pub tunnel_host: Option<String>,
    pub server_start_ms: u64,
    pub scan_complete: bool,
    pub scanned: usize,
    pub skipped: usize,
    pub filtered: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameInfo {
    pub frame_count: u32,
    pub rows: u32,
    pub columns: u32,
    pub transfer_syntax: String,
    pub has_pixels: bool,
    pub default_window: Option<WindowPreset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Default,
    FullDynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameCacheKey {
    pub file_index: usize,
    pub frame: u32,
    pub window_center_bits: Option<u64>,
    pub window_width_bits: Option<u64>,
    pub window_mode: WindowMode,
}

impl FrameCacheKey {
    pub fn new(
        file_index: usize,
        frame: u32,
        window_center: Option<f64>,
        window_width: Option<f64>,
        window_mode: WindowMode,
    ) -> Self {
        let (window_center, window_width) = match window_mode {
            WindowMode::Default => (window_center, window_width),
            WindowMode::FullDynamic => (None, None),
        };
        Self {
            file_index,
            frame,
            window_center_bits: window_center.map(f64::to_bits),
            window_width_bits: window_width.map(f64::to_bits),
            window_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRequest {
    center: Option<f64>,
    width: Option<f64>,
    mode: WindowMode,
}

impl WindowRequest {
    pub fn new(
        center: Option<f64>,
        width: Option<f64>,
        mode: WindowMode,
    ) -> Result<Self, WindowRequestError> {
        match (center, width) {
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {
                return Err(WindowRequestError::IncompletePair);
            }
            (Some(center), Some(width)) => {
                if !center.is_finite() {
                    return Err(WindowRequestError::NonFiniteCenter);
                }
                if !width.is_finite() {
                    return Err(WindowRequestError::NonFiniteWidth);
                }
                if width <= 0.0 {
                    return Err(WindowRequestError::NonPositiveWidth);
                }
            }
        }

        Ok(Self {
            center,
            width,
            mode,
        })
    }

    pub fn center(self) -> Option<f64> {
        self.center
    }

    pub fn width(self) -> Option<f64> {
        self.width
    }

    pub fn mode(self) -> WindowMode {
        self.mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRequestError {
    IncompletePair,
    NonFiniteCenter,
    NonFiniteWidth,
    NonPositiveWidth,
}

impl std::fmt::Display for WindowRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::IncompletePair => "window center and width must be provided together",
            Self::NonFiniteCenter => "window center must be finite",
            Self::NonFiniteWidth => "window width must be finite",
            Self::NonPositiveWidth => "window width must be greater than zero",
        })
    }
}

impl std::error::Error for WindowRequestError {}

#[cfg(test)]
mod tests {
    use super::{FrameCacheKey, WindowMode, WindowRequest, WindowRequestError};

    #[test]
    fn frame_cache_key_distinguishes_absent_and_explicit_window_params() {
        let default_window = FrameCacheKey::new(0, 0, None, None, WindowMode::Default);
        let explicit = FrameCacheKey::new(0, 0, Some(0.0), Some(1.0), WindowMode::Default);

        assert_ne!(default_window, explicit);
        assert_eq!(explicit.window_center_bits, Some(0));
        assert_eq!(explicit.window_width_bits, Some(1.0_f64.to_bits()));
    }

    #[test]
    fn full_dynamic_cache_key_ignores_explicit_window_values() {
        let first = FrameCacheKey::new(0, 0, Some(10.0), Some(20.0), WindowMode::FullDynamic);
        let second = FrameCacheKey::new(0, 0, Some(30.0), Some(40.0), WindowMode::FullDynamic);

        assert_eq!(first, second);
        assert_eq!(first.window_center_bits, None);
        assert_eq!(first.window_width_bits, None);
    }

    #[test]
    fn window_request_requires_a_finite_positive_pair() {
        assert_eq!(
            WindowRequest::new(Some(10.0), None, WindowMode::Default),
            Err(WindowRequestError::IncompletePair)
        );
        assert_eq!(
            WindowRequest::new(Some(f64::INFINITY), Some(20.0), WindowMode::Default),
            Err(WindowRequestError::NonFiniteCenter)
        );
        assert_eq!(
            WindowRequest::new(Some(10.0), Some(f64::NAN), WindowMode::Default),
            Err(WindowRequestError::NonFiniteWidth)
        );
        assert_eq!(
            WindowRequest::new(Some(10.0), Some(0.0), WindowMode::Default),
            Err(WindowRequestError::NonPositiveWidth)
        );
        assert!(WindowRequest::new(Some(10.0), Some(20.0), WindowMode::Default).is_ok());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TagNode {
    pub tag: String,
    pub vr: String,
    pub keyword: String,
    pub value: TagValue,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TagValue {
    String {
        value: String,
    },
    Number {
        value: f64,
    },
    Numbers {
        value: Vec<f64>,
        #[serde(skip_serializing_if = "is_false")]
        truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
    },
    Binary {
        length: usize,
    },
    Sequence {
        items: Vec<Vec<TagNode>>,
        #[serde(skip_serializing_if = "is_false")]
        truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
    },
    Error {
        message: String,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone)]
pub struct LoadReport {
    pub files: Vec<FileEntry>,
    pub skipped: usize,
    pub filtered: usize,
    pub searched_recursive: bool,
}

#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub tunnel_host: String,
    pub tunnel_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferSyntaxClass {
    Jpeg,
    JpegLossless,
    Jpeg2000,
    Uncompressed,
    JpegLs,
    Rle,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct ResolvedWindow {
    pub center: f64,
    pub width: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawFrameCacheKey {
    pub file_index: usize,
    pub frame: u32,
}

pub const RAW_FRAME_HEADER_ROWS: &str = "X-Frame-Rows";
pub const RAW_FRAME_HEADER_COLUMNS: &str = "X-Frame-Columns";
pub const RAW_FRAME_HEADER_BITS_ALLOCATED: &str = "X-Frame-Bits-Allocated";
pub const RAW_FRAME_HEADER_PIXEL_REPRESENTATION: &str = "X-Frame-Pixel-Representation";
pub const RAW_FRAME_HEADER_SAMPLES_PER_PIXEL: &str = "X-Frame-Samples-Per-Pixel";
pub const RAW_FRAME_HEADER_PHOTOMETRIC_INTERPRETATION: &str = "X-Frame-Photometric-Interpretation";
pub const RAW_FRAME_HEADER_RESCALE_SLOPE: &str = "X-Frame-Rescale-Slope";
pub const RAW_FRAME_HEADER_RESCALE_INTERCEPT: &str = "X-Frame-Rescale-Intercept";
pub const RAW_FRAME_HEADER_DEFAULT_WC: &str = "X-Frame-Default-Wc";
pub const RAW_FRAME_HEADER_DEFAULT_WW: &str = "X-Frame-Default-Ww";

#[derive(Debug, Clone, Serialize)]
pub struct RawFrameMetadata {
    pub rows: u32,
    pub columns: u32,
    pub bits_allocated: u32,
    pub pixel_representation: u32,
    pub samples_per_pixel: u32,
    pub photometric_interpretation: String,
    pub rescale_slope: f64,
    pub rescale_intercept: f64,
    pub default_wc: Option<f64>,
    pub default_ww: Option<f64>,
}
