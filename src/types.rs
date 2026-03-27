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
	pub has_pixels: bool,
	pub frame_count: u32,
	pub rows: u32,
	pub columns: u32,
	pub transfer_syntax_uid: String,
	pub default_window: Option<WindowPreset>,
	pub offset_table: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
	pub index: usize,
	pub path: String,
	pub label: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameCacheKey {
	pub file_index: usize,
	pub frame: u32,
	pub window_center_bits: u64,
	pub window_width_bits: u64,
}

impl FrameCacheKey {
	pub fn new(file_index: usize, frame: u32, window_center: Option<f64>, window_width: Option<f64>) -> Self {
		Self {
			file_index,
			frame,
			window_center_bits: window_center.map(f64::to_bits).unwrap_or(0),
			window_width_bits: window_width.map(f64::to_bits).unwrap_or(0),
		}
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
	String { value: String },
	Number { value: f64 },
	Numbers { value: Vec<f64> },
	Binary { length: usize },
	Sequence { items: Vec<Vec<TagNode>> },
	Error { message: String },
}

#[derive(Debug, Clone)]
pub struct LoadReport {
	pub files: Vec<FileEntry>,
	pub skipped: usize,
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
