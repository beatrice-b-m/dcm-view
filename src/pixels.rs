use crate::types::{FileEntry, FrameCacheKey, RawFrameCacheKey, RawFrameMetadata, ResolvedWindow, TransferSyntaxClass, WindowMode};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::collector::DicomCollector;
use dicom_object::open_file;
use dicom_pixeldata::PixelDecoder;
use image::{ImageBuffer, ImageFormat, Luma, Rgb};
use lru::LruCache;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::task;

pub const CACHE_CAPACITY: usize = 128;
pub const FRAME_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024; // 256 MiB
pub const RAW_CACHE_CAPACITY: usize = 512;
pub const RAW_CACHE_MAX_BYTES: usize = 384 * 1024 * 1024; // 384 MiB

pub fn new_cache() -> Arc<Mutex<LruCache<FrameCacheKey, Bytes>>> {
	Arc::new(Mutex::new(LruCache::new(
		NonZeroUsize::new(CACHE_CAPACITY).expect("non-zero cache capacity"),
	)))
}

pub fn new_raw_cache() -> Arc<Mutex<LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>>> {
	Arc::new(Mutex::new(LruCache::new(
		NonZeroUsize::new(RAW_CACHE_CAPACITY).expect("non-zero raw cache capacity"),
	)))
}

fn frame_cache_bytes(cache: &LruCache<FrameCacheKey, Bytes>) -> usize {
	cache.iter().map(|(_, body)| body.len()).sum()
}

fn raw_cache_bytes(cache: &LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>) -> usize {
	cache.iter().map(|(_, (body, _))| body.len()).sum()
}

fn cache_frame_with_budget(cache: &mut LruCache<FrameCacheKey, Bytes>, key: FrameCacheKey, body: Bytes) {
	cache_frame_with_budget_limit(cache, key, body, FRAME_CACHE_MAX_BYTES);
}

fn cache_frame_with_budget_limit(
	cache: &mut LruCache<FrameCacheKey, Bytes>,
	key: FrameCacheKey,
	body: Bytes,
	max_bytes: usize,
) {
	let incoming = body.len();
	if incoming > max_bytes {
		return;
	}

	cache.pop(&key);
	while frame_cache_bytes(cache).saturating_add(incoming) > max_bytes {
		if cache.pop_lru().is_none() {
			return;
		}
	}

	cache.put(key, body);
}

fn cache_raw_with_budget(
	cache: &mut LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>,
	key: RawFrameCacheKey,
	body: Bytes,
	metadata: RawFrameMetadata,
) {
	cache_raw_with_budget_limit(cache, key, body, metadata, RAW_CACHE_MAX_BYTES);
}

fn cache_raw_with_budget_limit(
	cache: &mut LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>,
	key: RawFrameCacheKey,
	body: Bytes,
	metadata: RawFrameMetadata,
	max_bytes: usize,
) {
	let incoming = body.len();
	if incoming > max_bytes {
		return;
	}

	cache.pop(&key);
	while raw_cache_bytes(cache).saturating_add(incoming) > max_bytes {
		if cache.pop_lru().is_none() {
			return;
		}
	}

	cache.put(key, (body, metadata));
}

#[derive(Debug, Clone)]
pub struct RawFrameRequest {
	pub file_index: usize,
	pub frame: u32,
}

#[derive(Debug, Clone)]
pub struct RawFrameResponse {
	pub body: Bytes,
	pub metadata: RawFrameMetadata,
	pub cache_hit: bool,
}

pub async fn load_raw_frame(
	files: &[FileEntry],
	cache: Arc<Mutex<LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>>>,
	request: RawFrameRequest,
) -> Result<RawFrameResponse> {
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
	if matches!(
		syntax_class,
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported
	) {
		return Err(anyhow!("unsupported transfer syntax: {}", file.transfer_syntax_uid));
	}

	let key = RawFrameCacheKey {
		file_index: request.file_index,
		frame: request.frame,
	};

	if let Ok(mut lock) = cache.lock() {
		if let Some((bytes, meta)) = lock.get(&key).cloned() {
			return Ok(RawFrameResponse {
				body: bytes,
				metadata: meta,
				cache_hit: true,
			});
		}
	}

	let (body, metadata) = match syntax_class {
		TransferSyntaxClass::Jpeg => {
			read_raw_jpeg_samples(file.path.clone(), request.frame, file.default_window).await?
		}
		TransferSyntaxClass::JpegLossless => {
			decode_raw_jpeg_lossless(file.path.clone(), request.frame, file.default_window).await?
		}
		TransferSyntaxClass::Jpeg2000 => {
			decode_raw_jp2_samples(file.path.clone(), request.frame, file.default_window).await?
		}
		TransferSyntaxClass::Uncompressed => {
			read_raw_uncompressed(file.path.clone(), request.frame, file.default_window).await?
		}
		_ => unreachable!("non-raw syntaxes filtered above"),
	};

	if let Ok(mut lock) = cache.lock() {
		cache_raw_with_budget(&mut lock, key, body.clone(), metadata.clone());
	}

	Ok(RawFrameResponse {
		body,
		metadata,
		cache_hit: false,
	})
}

#[derive(Debug, Clone)]
pub struct FrameRequest {
	pub file_index: usize,
	pub frame: u32,
	pub window_center: Option<f64>,
	pub window_width: Option<f64>,
	pub window_mode: WindowMode,
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
	// JP2 passthrough (raw fragment) is cheap and doesn't need caching.
	// JP2 decoded PNG must be cached — decode is expensive.
	// Caching both would cause key collisions (same key, different content types).
	let cache_allowed = !matches!(syntax_class, TransferSyntaxClass::Jpeg2000) || !jp2_accept;
	let key = FrameCacheKey::new(
		request.file_index,
		request.frame,
		request.window_center,
		request.window_width,
		request.window_mode,
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
		TransferSyntaxClass::JpegLossless => (
			decode_frame_to_png(file.path.clone(), request.frame).await?,
			"image/png",
		),
		TransferSyntaxClass::Jpeg2000 => {
			if jp2_accept {
				(
					read_encapsulated_fragment(file.path.clone(), request.frame).await?,
					"image/jp2",
				)
			} else {
				(
					decode_jp2_fragment_to_png(
						file.path.clone(),
						request.frame,
						request.window_center,
						request.window_width,
						file.default_window,
						request.window_mode,
					)
					.await?,
					"image/png",
				)
			}
		}
		TransferSyntaxClass::Uncompressed => (
			decode_uncompressed_to_png(
				file.path.clone(),
				request.frame,
				request.window_center,
				request.window_width,
				file.default_window,
				request.window_mode,
			)
			.await?,
			"image/png",
		),
		TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported => {
			return Err(anyhow!(
				"unsupported transfer syntax: {}",
				file.transfer_syntax_uid
			));
		}
	};

	if cache_allowed {
		if let Ok(mut lock) = cache.lock() {
			cache_frame_with_budget(&mut lock, key, body.clone());
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
		TransferSyntaxClass::JpegLossless => "image/png",
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

async fn decode_jp2_fragment_to_png(
	path: PathBuf,
	frame: u32,
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	window_mode: WindowMode,
) -> Result<Bytes> {
	task::spawn_blocking(move || {
		decode_jp2_fragment_to_png_blocking(&path, frame, requested_wc, requested_ww, default_window, window_mode)
	})
	.await
	.context("jp2 fragment decode task failed")?
}

fn decode_jp2_fragment_to_png_blocking(
	path: &PathBuf,
	frame: u32,
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	window_mode: WindowMode,
) -> Result<Bytes> {
	let fragment = read_encapsulated_fragment_blocking(path, frame)?;

	let jp2_image = jpeg2k::Image::from_bytes(&fragment)
		.map_err(anyhow::Error::from)
		.context("failed to decode JP2 fragment")?;

	let comps = jp2_image.components();
	if comps.is_empty() {
		return Err(anyhow!("JP2 image has no components"));
	}

	let mut buffer = Cursor::new(Vec::<u8>::new());

	if comps.len() == 1 {
		// Grayscale — the common medical imaging case
		let width = comps[0].width();
		let height = comps[0].height();
		let raw_samples: Vec<f64> = comps[0].data().iter().map(|&v| v as f64).collect();
		let resolved_window =
			resolve_window_with_mode(window_mode, requested_wc, requested_ww, default_window, &raw_samples)
				.ok_or_else(|| anyhow!("JP2 decode failed: could not resolve window"))?;
		let windowed = apply_window(&raw_samples, resolved_window.center, resolved_window.width.max(1.0));
		let image = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(width, height, windowed)
			.ok_or_else(|| anyhow!("JP2 decoded buffer size mismatch"))?;
		image::DynamicImage::ImageLuma8(image)
			.write_to(&mut buffer, ImageFormat::Png)
			.context("JP2 decode failed: png encoding failed")?;
	} else if comps.len() == 3 {
		// RGB — rare in medical imaging but handle it
		let width = comps[0].width();
		let height = comps[0].height();
		let precision = comps[0].precision();
		if precision <= 8 {
			let r = comps[0].data_u8();
			let g = comps[1].data_u8();
			let b = comps[2].data_u8();
			let interleaved: Vec<u8> = r.zip(g).zip(b)
				.flat_map(|((rv, gv), bv)| [rv, gv, bv])
				.collect();
			let image = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(width, height, interleaved)
				.ok_or_else(|| anyhow!("JP2 decoded buffer size mismatch"))?;
			image::DynamicImage::ImageRgb8(image)
				.write_to(&mut buffer, ImageFormat::Png)
				.context("JP2 decode failed: png encoding failed")?;
		} else if precision <= 16 {
			let r = comps[0].data_u16();
			let g = comps[1].data_u16();
			let b = comps[2].data_u16();
			let interleaved: Vec<u16> = r.zip(g).zip(b)
				.flat_map(|((rv, gv), bv)| [rv, gv, bv])
				.collect();
			let image = ImageBuffer::<Rgb<u16>, Vec<u16>>::from_raw(width, height, interleaved)
				.ok_or_else(|| anyhow!("JP2 decoded buffer size mismatch"))?;
			image::DynamicImage::ImageRgb16(image)
				.write_to(&mut buffer, ImageFormat::Png)
				.context("JP2 decode failed: png encoding failed")?;
		} else {
			return Err(anyhow!("unsupported JP2 component layout"));
		}
	} else {
		return Err(anyhow!("unsupported JP2 component layout"));
	}

	Ok(Bytes::from(buffer.into_inner()))
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

async fn decode_uncompressed_to_png(
	path: PathBuf,
	frame: u32,
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	window_mode: WindowMode,
) -> Result<Bytes> {
	task::spawn_blocking(move || {
		decode_uncompressed_to_png_blocking(
			&path,
			frame,
			requested_wc,
			requested_ww,
			default_window,
			window_mode,
		)
	})
	.await
	.context("uncompressed decode task failed")?
}

fn decode_uncompressed_to_png_blocking(
	path: &PathBuf,
	frame: u32,
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	window_mode: WindowMode,
) -> Result<Bytes> {
	let object = open_file(path)
		.with_context(|| format!("failed to open DICOM for uncompressed decode: {}", path.display()))?;

	let rows = read_u32_tag(&object, "Rows").unwrap_or(0);
	let columns = read_u32_tag(&object, "Columns").unwrap_or(0);
	let samples_per_pixel = read_u32_tag(&object, "SamplesPerPixel").unwrap_or(1).max(1);
	let bits_allocated = read_u32_tag(&object, "BitsAllocated").unwrap_or(8);
	let pixel_representation = read_u32_tag(&object, "PixelRepresentation").unwrap_or(0);
	let slope = read_f64_tag(&object, "RescaleSlope").unwrap_or(1.0);
	let intercept = read_f64_tag(&object, "RescaleIntercept").unwrap_or(0.0);

	let bytes_per_sample = (bits_allocated / 8) as usize;
	if rows == 0 || columns == 0 || bytes_per_sample == 0 {
		return Err(anyhow!("frame decode failed: invalid image geometry"));
	}

	let frame_size = rows as usize
		* columns as usize
		* samples_per_pixel as usize
		* bytes_per_sample;
	let offset = frame as usize * frame_size;

	let pixel_bytes = object
		.element_by_name("PixelData")
		.context("frame decode failed: missing PixelData")?
		.to_bytes()
		.context("frame decode failed: pixel bytes unavailable")?
		.into_owned();

	if offset + frame_size > pixel_bytes.len() {
		return Err(anyhow!("frame out of range"));
	}

	let frame_slice = &pixel_bytes[offset..offset + frame_size];
	let signed = pixel_representation == 1;
	// dicom-object normalizes primitive pixel bytes to host order for native pixel data.
	// Decode from the normalized byte representation directly.
	let raw_samples = decode_numeric_samples(frame_slice, bits_allocated, signed, false)?;
	let rescaled: Vec<f64> = raw_samples
		.into_iter()
		.map(|value| value * slope + intercept)
		.collect();

	let luminance_samples = if samples_per_pixel > 1 {
		rescaled
			.chunks(samples_per_pixel as usize)
			.map(|chunk| chunk[0])
			.collect::<Vec<_>>()
	} else {
		rescaled
	};

	let resolved_window = resolve_window_with_mode(window_mode, requested_wc, requested_ww, default_window, &luminance_samples)
		.ok_or_else(|| anyhow!("frame decode failed: could not resolve window"))?;
	let windowed = apply_window(
		&luminance_samples,
		resolved_window.center,
		resolved_window.width.max(1.0),
	);

	let image = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(columns, rows, windowed)
		.ok_or_else(|| anyhow!("frame decode failed: windowed buffer size mismatch"))?;
	let mut encoded = Cursor::new(Vec::<u8>::new());
	image::DynamicImage::ImageLuma8(image)
		.write_to(&mut encoded, ImageFormat::Png)
		.context("frame decode failed: png encoding failed")?;

	Ok(Bytes::from(encoded.into_inner()))
}

fn decode_numeric_samples(
	frame_slice: &[u8],
	bits_allocated: u32,
	signed: bool,
	big_endian: bool,
) -> Result<Vec<f64>> {
	match (bits_allocated, signed) {
		(8, false) => Ok(frame_slice.iter().map(|value| *value as f64).collect()),
		(8, true) => Ok(frame_slice.iter().map(|value| (*value as i8) as f64).collect()),
		(16, false) => {
			let mut out = Vec::with_capacity(frame_slice.len() / 2);
			for chunk in frame_slice.chunks_exact(2) {
				let value = if big_endian {
					u16::from_be_bytes([chunk[0], chunk[1]])
				} else {
					u16::from_le_bytes([chunk[0], chunk[1]])
				};
				out.push(value as f64);
			}
			Ok(out)
		}
		(16, true) => {
			let mut out = Vec::with_capacity(frame_slice.len() / 2);
			for chunk in frame_slice.chunks_exact(2) {
				let value = if big_endian {
					i16::from_be_bytes([chunk[0], chunk[1]])
				} else {
					i16::from_le_bytes([chunk[0], chunk[1]])
				};
				out.push(value as f64);
			}
			Ok(out)
		}
		_ => Err(anyhow!(
			"frame decode failed: unsupported BitsAllocated {bits_allocated} for uncompressed path"
		)),
	}
}

fn read_u32_tag(object: &dicom_object::DefaultDicomObject, name: &str) -> Option<u32> {
	object
		.element_by_name(name)
		.ok()?
		.to_str()
		.ok()
		.and_then(|value| value.split('\\').next().map(str::trim).map(str::to_string))
		.and_then(|value| value.parse::<u32>().ok())
}

fn read_f64_tag(object: &dicom_object::DefaultDicomObject, name: &str) -> Option<f64> {
	object
		.element_by_name(name)
		.ok()?
		.to_str()
		.ok()
		.and_then(|value| value.split('\\').next().map(str::trim).map(str::to_string))
		.and_then(|value| value.parse::<f64>().ok())
}

fn read_str_tag(object: &dicom_object::DefaultDicomObject, name: &str) -> Option<String> {
	object
		.element_by_name(name)
		.ok()?
		.to_str()
		.ok()
		.map(|v| v.trim().to_string())
}

async fn read_raw_uncompressed(
	path: PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	task::spawn_blocking(move || read_raw_uncompressed_blocking(&path, frame, default_window))
		.await
		.context("raw uncompressed read task failed")?
}

fn read_raw_uncompressed_blocking(
	path: &PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	let object = open_file(path)
		.with_context(|| format!("failed to open DICOM for raw uncompressed read: {}", path.display()))?;

	let rows = read_u32_tag(&object, "Rows").unwrap_or(0);
	let columns = read_u32_tag(&object, "Columns").unwrap_or(0);
	let samples_per_pixel = read_u32_tag(&object, "SamplesPerPixel").unwrap_or(1).max(1);
	let bits_allocated = read_u32_tag(&object, "BitsAllocated").unwrap_or(8);
	let pixel_representation = read_u32_tag(&object, "PixelRepresentation").unwrap_or(0);
	let photometric_interpretation =
		read_str_tag(&object, "PhotometricInterpretation").unwrap_or_else(|| "MONOCHROME2".to_string());
	let slope = read_f64_tag(&object, "RescaleSlope").unwrap_or(1.0);
	let intercept = read_f64_tag(&object, "RescaleIntercept").unwrap_or(0.0);

	let bytes_per_sample = (bits_allocated / 8).max(1) as usize;
	if rows == 0 || columns == 0 {
		return Err(anyhow!("frame decode failed: invalid image geometry"));
	}

	let frame_size = rows as usize * columns as usize * samples_per_pixel as usize * bytes_per_sample;
	let offset = frame as usize * frame_size;

	let pixel_bytes = object
		.element_by_name("PixelData")
		.context("frame decode failed: missing PixelData")?
		.to_bytes()
		.context("frame decode failed: pixel bytes unavailable")?;

	if offset + frame_size > pixel_bytes.len() {
		return Err(anyhow!("frame out of range"));
	}

	// dicom-object normalizes pixel bytes to host (LE) order — slice is already LE.
	let frame_bytes = pixel_bytes[offset..offset + frame_size].to_vec();

	let metadata = RawFrameMetadata {
		rows,
		columns,
		bits_allocated,
		pixel_representation,
		samples_per_pixel,
		photometric_interpretation,
		rescale_slope: slope,
		rescale_intercept: intercept,
		default_wc: default_window.map(|w| w.center),
		default_ww: default_window.map(|w| w.width),
	};
	Ok((Bytes::from(frame_bytes), metadata))
}

async fn read_raw_jpeg_samples(
	path: PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	task::spawn_blocking(move || read_raw_jpeg_samples_blocking(&path, frame, default_window))
		.await
		.context("raw JPEG sample read task failed")?
}

fn read_raw_jpeg_samples_blocking(
	path: &PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	let fragment = read_encapsulated_fragment_blocking(path, frame)?;
	// Decode JPEG to 8-bit grayscale samples. Tolerates Baseline and Extended JPEG.
	let img = image::load_from_memory(&fragment)
		.context("JPEG decode failed for raw samples")?
		.to_luma8();
	let (columns, rows) = (img.width(), img.height());
	let samples = img.into_raw();

	// Read metadata tags for context (photometric, rescale, etc.).
	let object = open_file(path)
		.with_context(|| format!("failed to open DICOM for raw JPEG metadata: {}", path.display()))?;
	let photometric_interpretation =
		read_str_tag(&object, "PhotometricInterpretation").unwrap_or_else(|| "MONOCHROME2".to_string());
	let slope = read_f64_tag(&object, "RescaleSlope").unwrap_or(1.0);
	let intercept = read_f64_tag(&object, "RescaleIntercept").unwrap_or(0.0);

	let metadata = RawFrameMetadata {
		rows,
		columns,
		bits_allocated: 8,
		pixel_representation: 0,
		samples_per_pixel: 1,
		photometric_interpretation,
		rescale_slope: slope,
		rescale_intercept: intercept,
		default_wc: default_window.map(|w| w.center),
		default_ww: default_window.map(|w| w.width),
	};
	Ok((Bytes::from(samples), metadata))
}

async fn decode_raw_jpeg_lossless(
	path: PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	task::spawn_blocking(move || decode_raw_jpeg_lossless_blocking(&path, frame, default_window))
		.await
		.context("raw JPEG Lossless decode task failed")?
}

fn decode_raw_jpeg_lossless_blocking(
	path: &PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	let obj = open_file(path)
		.with_context(|| format!("failed to open DICOM for raw JPEG Lossless decode: {}", path.display()))?;

	let photometric =
		read_str_tag(&obj, "PhotometricInterpretation").unwrap_or_else(|| "MONOCHROME2".to_string());
	let pixel_representation = read_u32_tag(&obj, "PixelRepresentation").unwrap_or(0);
	let slope = read_f64_tag(&obj, "RescaleSlope").unwrap_or(1.0);
	let intercept = read_f64_tag(&obj, "RescaleIntercept").unwrap_or(0.0);

	let decoded = obj
		.decode_pixel_data()
		.with_context(|| format!("unsupported transfer syntax: {}", obj.meta().transfer_syntax()))?;
	let img = decoded
		.to_dynamic_image(frame)
		.with_context(|| format!("unsupported transfer syntax: {}", obj.meta().transfer_syntax()))?;

	let (img_rows, img_columns) = (img.height(), img.width());

	let (sample_bytes, bits_allocated) = match img {
		image::DynamicImage::ImageLuma8(luma8) => {
			let samples = luma8.into_raw();
			(Bytes::from(samples), 8u32)
		}
		image::DynamicImage::ImageLuma16(luma16) => {
			let bytes: Vec<u8> = luma16.into_raw().iter().flat_map(|&v| v.to_le_bytes()).collect();
			(Bytes::from(bytes), 16u32)
		}
		other => {
			// Convert non-grayscale to grayscale (luma8) as fallback
			let luma8 = other.into_luma8();
			let samples = luma8.into_raw();
			(Bytes::from(samples), 8u32)
		}
	};

	let metadata = RawFrameMetadata {
		rows: img_rows,
		columns: img_columns,
		bits_allocated,
		pixel_representation,
		samples_per_pixel: 1,
		photometric_interpretation: photometric,
		rescale_slope: slope,
		rescale_intercept: intercept,
		default_wc: default_window.map(|w| w.center),
		default_ww: default_window.map(|w| w.width),
	};
	Ok((sample_bytes, metadata))
}

async fn decode_raw_jp2_samples(
	path: PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	task::spawn_blocking(move || decode_raw_jp2_samples_blocking(&path, frame, default_window))
		.await
		.context("raw JP2 decode task failed")?
}

fn decode_raw_jp2_samples_blocking(
	path: &PathBuf,
	frame: u32,
	default_window: Option<crate::types::WindowPreset>,
) -> Result<(Bytes, RawFrameMetadata)> {
	let fragment = read_encapsulated_fragment_blocking(path, frame)?;

	let jp2_image = jpeg2k::Image::from_bytes(&fragment)
		.map_err(anyhow::Error::from)
		.context("failed to decode JP2 fragment for raw samples")?;

	let comps = jp2_image.components();
	if comps.is_empty() {
		return Err(anyhow!("JP2 image has no components"));
	}

	// Only grayscale (single component) is supported for the raw path.
	// Multi-component (RGB) JP2 images are rare in DICOM and are 422 here.
	if comps.len() != 1 {
		return Err(anyhow!("unsupported JP2 component layout for raw decode"));
	}

	let width = comps[0].width();
	let height = comps[0].height();
	let precision = comps[0].precision();

	let object = open_file(path)
		.with_context(|| format!("failed to open DICOM for raw JP2 metadata: {}", path.display()))?;
	let photometric =
		read_str_tag(&object, "PhotometricInterpretation").unwrap_or_else(|| "MONOCHROME2".to_string());
	let pixel_representation = read_u32_tag(&object, "PixelRepresentation").unwrap_or(0);
	let slope = read_f64_tag(&object, "RescaleSlope").unwrap_or(1.0);
	let intercept = read_f64_tag(&object, "RescaleIntercept").unwrap_or(0.0);

	let (sample_bytes, bits_allocated) = if precision <= 8 {
		let samples: Vec<u8> = comps[0].data_u8().collect();
		(Bytes::from(samples), 8u32)
	} else {
		// Normalize to u16 LE for any precision 9-16.
		let bytes: Vec<u8> = comps[0].data_u16().flat_map(|v| v.to_le_bytes()).collect();
		(Bytes::from(bytes), 16u32)
	};

	let metadata = RawFrameMetadata {
		rows: height,
		columns: width,
		bits_allocated,
		pixel_representation,
		samples_per_pixel: 1,
		photometric_interpretation: photometric,
		rescale_slope: slope,
		rescale_intercept: intercept,
		default_wc: default_window.map(|w| w.center),
		default_ww: default_window.map(|w| w.width),
	};
	Ok((sample_bytes, metadata))
}

pub fn classify_transfer_syntax(uid: &str) -> TransferSyntaxClass {
	match uid {
		// Browser-renderable lossy JPEG: Baseline, Extended
		"1.2.840.10008.1.2.4.50"
		| "1.2.840.10008.1.2.4.51" => TransferSyntaxClass::Jpeg,
		// JPEG Lossless: browsers cannot decode — must be decoded server-side
		"1.2.840.10008.1.2.4.57"
		| "1.2.840.10008.1.2.4.70" => TransferSyntaxClass::JpegLossless,
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

/// Computes window from the true min/max of frame samples (full dynamic range).
/// Ignores explicit wc/ww params and DICOM default_window tags.
fn full_dynamic_window(samples: &[f64]) -> Option<ResolvedWindow> {
	if samples.is_empty() {
		return None;
	}
	let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
	let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
	let width = (max - min).max(1.0);
	let center = min + width / 2.0;
	Some(ResolvedWindow { center, width })
}

/// Resolves window using the specified mode.
/// Default mode: explicit params -> DICOM default_window -> 1st/99th percentile.
/// FullDynamic mode: true min/max of current frame samples, ignores all other inputs.
pub fn resolve_window_with_mode(
	mode: WindowMode,
	requested_wc: Option<f64>,
	requested_ww: Option<f64>,
	default_window: Option<crate::types::WindowPreset>,
	samples: &[f64],
) -> Option<ResolvedWindow> {
	match mode {
		WindowMode::Default => resolve_window(requested_wc, requested_ww, default_window, samples),
		WindowMode::FullDynamic => full_dynamic_window(samples),
	}
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


#[cfg(test)]
mod tests {
	use super::*;

	fn frame_key(frame: u32) -> FrameCacheKey {
		FrameCacheKey::new(0, frame, None, None, WindowMode::Default)
	}

	fn raw_key(frame: u32) -> RawFrameCacheKey {
		RawFrameCacheKey { file_index: 0, frame }
	}

	fn raw_meta() -> RawFrameMetadata {
		RawFrameMetadata {
			rows: 1,
			columns: 1,
			bits_allocated: 8,
			pixel_representation: 0,
			samples_per_pixel: 1,
			photometric_interpretation: "MONOCHROME2".to_string(),
			rescale_slope: 1.0,
			rescale_intercept: 0.0,
			default_wc: None,
			default_ww: None,
		}
	}

	fn frame_cache_contains(cache: &LruCache<FrameCacheKey, Bytes>, key: &FrameCacheKey) -> bool {
		cache.iter().any(|(cached_key, _)| cached_key == key)
	}

	fn raw_cache_contains(cache: &LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>, key: &RawFrameCacheKey) -> bool {
		cache.iter().any(|(cached_key, _)| cached_key == key)
	}

	#[test]
	fn frame_cache_budget_evicts_lru_entries() {
		let mut cache = LruCache::new(NonZeroUsize::new(4).expect("non-zero cache size"));
		let key0 = frame_key(0);
		let key1 = frame_key(1);
		let key2 = frame_key(2);

		cache_frame_with_budget_limit(&mut cache, key0.clone(), Bytes::from(vec![0_u8; 4]), 8);
		cache_frame_with_budget_limit(&mut cache, key1.clone(), Bytes::from(vec![1_u8; 4]), 8);
		cache_frame_with_budget_limit(&mut cache, key2.clone(), Bytes::from(vec![2_u8; 4]), 8);

		assert!(!frame_cache_contains(&cache, &key0), "least-recently-used entry should be evicted");
		assert!(frame_cache_contains(&cache, &key1), "second entry should still be cached");
		assert!(frame_cache_contains(&cache, &key2), "new entry should be cached");
	}

	#[test]
	fn frame_cache_budget_skips_oversized_entries() {
		let mut cache = LruCache::new(NonZeroUsize::new(4).expect("non-zero cache size"));
		let key0 = frame_key(0);

		cache_frame_with_budget_limit(&mut cache, key0.clone(), Bytes::from(vec![0_u8; 9]), 8);

		assert!(!frame_cache_contains(&cache, &key0), "oversized entry should be skipped");
	}

	#[test]
	fn raw_cache_budget_evicts_lru_entries() {
		let mut cache = LruCache::new(NonZeroUsize::new(4).expect("non-zero cache size"));
		let key0 = raw_key(0);
		let key1 = raw_key(1);
		let key2 = raw_key(2);

		cache_raw_with_budget_limit(&mut cache, key0.clone(), Bytes::from(vec![0_u8; 4]), raw_meta(), 8);
		cache_raw_with_budget_limit(&mut cache, key1.clone(), Bytes::from(vec![1_u8; 4]), raw_meta(), 8);
		cache_raw_with_budget_limit(&mut cache, key2.clone(), Bytes::from(vec![2_u8; 4]), raw_meta(), 8);

		assert!(!raw_cache_contains(&cache, &key0), "least-recently-used raw entry should be evicted");
		assert!(raw_cache_contains(&cache, &key1), "second raw entry should still be cached");
		assert!(raw_cache_contains(&cache, &key2), "new raw entry should be cached");
	}
}