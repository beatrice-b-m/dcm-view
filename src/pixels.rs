use crate::types::{ErrorResponse, FileEntry, FrameCacheKey, ResolvedWindow, TransferSyntaxClass};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::collector::DicomCollector;
use dicom_object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::ImageFormat;
use lru::LruCache;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::task;

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

	let syntax_class = classify_transfer_syntax(&file.transfer_syntax_uid);
	let jp2_accept = accepts_jp2(request.accept_header.as_deref());
	let cache_allowed = !matches!(syntax_class, TransferSyntaxClass::Jpeg2000) || jp2_accept;
	let key = FrameCacheKey::new(
		request.file_index,
		request.frame,
		request.window_center,
		request.window_width,
	);

	if cache_allowed {
		if let Ok(mut lock) = cache.lock() {
			if let Some(bytes) = lock.get(&key).cloned() {
				return Ok(FrameResponse {
					body: bytes,
					content_type: content_type_for_class(syntax_class, jp2_accept),
					cache_hit: true,
				});
			}
		}
	}

	let (body, content_type) = match syntax_class {
		TransferSyntaxClass::Jpeg => (
			read_encapsulated_fragment(file.path.clone(), request.frame).await?,
			"image/jpeg",
		),
		TransferSyntaxClass::Jpeg2000 => {
			if jp2_accept {
				(
					read_encapsulated_fragment(file.path.clone(), request.frame).await?,
					"image/jp2",
				)
			} else {
				(
					decode_frame_to_png(file.path.clone(), request.frame).await?,
					"image/png",
				)
			}
		}
		TransferSyntaxClass::Uncompressed => {
			return Err(anyhow!("frame decode failed: uncompressed decode not implemented yet"));
		}
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported => {
			return Err(anyhow!(
				"unsupported transfer syntax: {}",
				file.transfer_syntax_uid
			));
		}
	};

	if cache_allowed {
		if let Ok(mut lock) = cache.lock() {
			lock.put(key, body.clone());
		}
	}

	Ok(FrameResponse {
		body,
		content_type,
		cache_hit: false,
	})
}

fn content_type_for_class(class: TransferSyntaxClass, jp2_accept: bool) -> &'static str {
	match class {
		TransferSyntaxClass::Jpeg => "image/jpeg",
		TransferSyntaxClass::Jpeg2000 if jp2_accept => "image/jp2",
		TransferSyntaxClass::Jpeg2000 => "image/png",
		TransferSyntaxClass::Uncompressed => "image/png",
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported => {
			"application/octet-stream"
		}
	}
}

fn accepts_jp2(accept_header: Option<&str>) -> bool {
	accept_header
		.map(|value| value.split(',').any(|part| part.trim().starts_with("image/jp2")))
		.unwrap_or(false)
}

async fn read_encapsulated_fragment(path: PathBuf, frame: u32) -> Result<Bytes> {
	task::spawn_blocking(move || read_encapsulated_fragment_blocking(&path, frame))
		.await
		.context("fragment reader task failed")?
}

fn read_encapsulated_fragment_blocking(path: &PathBuf, frame: u32) -> Result<Bytes> {
	let mut collector = DicomCollector::open_file(path)
		.with_context(|| format!("failed to open DICOM for collector access: {}", path.display()))?;

	let mut offset_table = Vec::<u32>::new();
	let _ = collector.read_basic_offset_table(&mut offset_table)?;
	if offset_table.iter().all(|offset| *offset == 0) {
		offset_table.clear();
	}

	let mut fragment = Vec::<u8>::new();
	for _ in 0..=frame {
		fragment.clear();
		collector
			.read_next_fragment(&mut fragment)?
			.ok_or_else(|| anyhow!("frame out of range"))?;
	}

	Ok(Bytes::from(fragment))
}

async fn decode_frame_to_png(path: PathBuf, frame: u32) -> Result<Bytes> {
	task::spawn_blocking(move || decode_frame_to_png_blocking(&path, frame))
		.await
		.context("jp2 fallback decode task failed")?
}

fn decode_frame_to_png_blocking(path: &PathBuf, frame: u32) -> Result<Bytes> {
	let obj = open_file(path)
		.with_context(|| format!("failed to open DICOM for decode fallback: {}", path.display()))?;
	let decoded = obj
		.decode_pixel_data()
		.with_context(|| format!("unsupported transfer syntax: {}", obj.meta().transfer_syntax()))?;
	let image = decoded.to_dynamic_image(frame).with_context(|| {
		format!("unsupported transfer syntax: {}", obj.meta().transfer_syntax())
	})?;

	let mut buffer = Cursor::new(Vec::<u8>::new());
	image
		.write_to(&mut buffer, ImageFormat::Png)
		.context("failed to encode PNG")?;
	Ok(Bytes::from(buffer.into_inner()))
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
