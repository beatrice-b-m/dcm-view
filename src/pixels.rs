use crate::types::{ErrorResponse, FileEntry, FrameCacheKey, ResolvedWindow, TransferSyntaxClass};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

pub const CACHE_CAPACITY: usize = 128;

pub fn new_cache() -> Arc<Mutex<LruCache<FrameCacheKey, Bytes>>> {
	Arc::new(Mutex::new(LruCache::new(
		NonZeroUsize::new(CACHE_CAPACITY).expect("non-zero cache capacity"),
	)))
}

#[derive(Debug, Clone)]
pub struct FrameRequest {
	pub file_index: usize,
	pub frame: u32,
	pub window_center: Option<f64>,
	pub window_width: Option<f64>,
	pub accept_header: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FrameResponse {
	pub body: Bytes,
	pub content_type: &'static str,
	pub cache_hit: bool,
}

pub async fn load_frame(
	files: &[FileEntry],
	cache: Arc<Mutex<LruCache<FrameCacheKey, Bytes>>>,
	request: FrameRequest,
) -> Result<FrameResponse> {
	let file = files
		.get(request.file_index)
		.ok_or_else(|| anyhow!("file index out of range"))?;

	if !file.has_pixels {
		return Err(anyhow!("no pixel data"));
	}
	if request.frame >= file.frame_count {
		return Err(anyhow!("frame out of range"));
	}

	let key = FrameCacheKey::new(
		request.file_index,
		request.frame,
		request.window_center,
		request.window_width,
	);

	if let Ok(mut lock) = cache.lock() {
		if let Some(bytes) = lock.get(&key).cloned() {
			return Ok(FrameResponse {
				body: bytes,
				content_type: classify_content_type(file, request.accept_header.as_deref())?,
				cache_hit: true,
			});
		}
	}

	let bytes = placeholder_frame(file, &request)?;
	let content_type = classify_content_type(file, request.accept_header.as_deref())?;

	if let Ok(mut lock) = cache.lock() {
		lock.put(key, bytes.clone());
	}

	Ok(FrameResponse {
		body: bytes,
		content_type,
		cache_hit: false,
	})
}

fn placeholder_frame(file: &FileEntry, _request: &FrameRequest) -> Result<Bytes> {
	match classify_transfer_syntax(&file.transfer_syntax_uid) {
		TransferSyntaxClass::Jpeg | TransferSyntaxClass::Jpeg2000 => {
			Err(anyhow!("frame decode failed: passthrough not implemented yet"))
		}
		TransferSyntaxClass::Uncompressed => {
			Err(anyhow!("frame decode failed: uncompressed decode not implemented yet"))
		}
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle => {
			Err(anyhow!("unsupported transfer syntax: {}", file.transfer_syntax_uid))
		}
		TransferSyntaxClass::Unsupported => {
			Err(anyhow!("unsupported transfer syntax: {}", file.transfer_syntax_uid))
		}
	}
}

fn classify_content_type(file: &FileEntry, accept_header: Option<&str>) -> Result<&'static str> {
	match classify_transfer_syntax(&file.transfer_syntax_uid) {
		TransferSyntaxClass::Jpeg => Ok("image/jpeg"),
		TransferSyntaxClass::Jpeg2000 => {
			if accept_header.map(|value| value.contains("image/jp2")).unwrap_or(false) {
				Ok("image/jp2")
			} else {
				Ok("image/png")
			}
		}
		TransferSyntaxClass::Uncompressed => Ok("image/png"),
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported => {
			Err(anyhow!("unsupported transfer syntax: {}", file.transfer_syntax_uid))
		}
	}
}

pub fn classify_transfer_syntax(uid: &str) -> TransferSyntaxClass {
	match uid {
		"1.2.840.10008.1.2.4.50"
		| "1.2.840.10008.1.2.4.51"
		| "1.2.840.10008.1.2.4.57"
		| "1.2.840.10008.1.2.4.70" => TransferSyntaxClass::Jpeg,
		"1.2.840.10008.1.2.4.90" | "1.2.840.10008.1.2.4.91" => TransferSyntaxClass::Jpeg2000,
		"1.2.840.10008.1.2" | "1.2.840.10008.1.2.1" | "1.2.840.10008.1.2.2" => {
			TransferSyntaxClass::Uncompressed
		}
		"1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => TransferSyntaxClass::JpegLs,
		"1.2.840.10008.1.2.5" => TransferSyntaxClass::Rle,
		_ => TransferSyntaxClass::Unsupported,
	}
}

pub fn resolve_window(
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	samples: &[f64],
) -> Option<ResolvedWindow> {
	if let (Some(center), Some(width)) = (requested_wc, requested_ww) {
		return Some(ResolvedWindow { center, width });
	}

	if let Some(window) = default_window {
		return Some(ResolvedWindow {
			center: window.center,
			width: window.width,
		});
	}

	percentile_window(samples)
}

fn percentile_window(samples: &[f64]) -> Option<ResolvedWindow> {
	if samples.is_empty() {
		return None;
	}

	let mut values = samples.to_vec();
	values.sort_by(f64::total_cmp);
	let p1_idx = ((values.len() as f64) * 0.01).floor() as usize;
	let p99_idx = (((values.len() as f64) * 0.99).ceil() as usize).min(values.len().saturating_sub(1));
	let low = values[p1_idx.min(values.len().saturating_sub(1))];
	let high = values[p99_idx];
	let width = (high - low).max(1.0);
	let center = low + (width / 2.0);
	Some(ResolvedWindow { center, width })
}

pub fn apply_window(samples: &[f64], center: f64, width: f64) -> Vec<u8> {
	let low = center - width / 2.0;
	let high = center + width / 2.0;
	samples
		.iter()
		.map(|sample| (((sample.clamp(low, high) - low) / (high - low)) * 255.0).round() as u8)
		.collect()
}

pub fn unsupported_ts_error(uid: &str) -> ErrorResponse {
	ErrorResponse {
		error: format!("unsupported transfer syntax: {uid}"),
	}
}
