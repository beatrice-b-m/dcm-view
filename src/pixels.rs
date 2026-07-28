use crate::types::{
    FileEntry, FrameCacheKey, RawFrameCacheKey, RawFrameMetadata, ResolvedWindow,
    TransferSyntaxClass, WindowMode, WindowRequest,
};
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
use thiserror::Error;
use tokio::task;

pub const CACHE_CAPACITY: usize = 128;
// Keep these budgets in sync with README memory guidance and frontend frame retention.
pub const FRAME_CACHE_MAX_BYTES: usize = 256 * 1024 * 1024; // 256 MiB
pub const RAW_CACHE_CAPACITY: usize = 512;
pub const RAW_CACHE_MAX_BYTES: usize = 384 * 1024 * 1024; // 384 MiB

#[derive(Debug, Error)]
pub enum PixelError {
    #[error("no pixel data")]
    NoPixelData,
    #[error("frame out of range")]
    FrameOutOfRange,
    #[error("unsupported transfer syntax: {0}")]
    UnsupportedTransferSyntax(String),
    #[error("unsupported pixel layout: {0}")]
    UnsupportedLayout(String),
    #[error("invalid window request: {0}")]
    InvalidWindow(String),
    #[error("{context}: {source}")]
    Decode {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

impl PixelError {
    fn frame_decode(source: anyhow::Error) -> Self {
        Self::Decode {
            context: "frame decode failed",
            source,
        }
    }

    fn raw_decode(source: anyhow::Error) -> Self {
        Self::Decode {
            context: "raw frame decode failed",
            source,
        }
    }
}

pub type PixelResult<T> = std::result::Result<T, PixelError>;

pub struct FrameCache {
    entries: LruCache<FrameCacheKey, Bytes>,
    bytes: usize,
}

impl FrameCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(capacity).expect("non-zero cache capacity")),
            bytes: 0,
        }
    }

    fn get(&mut self, key: &FrameCacheKey) -> Option<Bytes> {
        self.entries.get(key).cloned()
    }

    fn insert_with_budget(&mut self, key: FrameCacheKey, body: Bytes, max_bytes: usize) {
        let incoming = body.len();
        if incoming > max_bytes {
            return;
        }

        if let Some(existing) = self.entries.pop(&key) {
            self.bytes = self.bytes.saturating_sub(existing.len());
        }

        while self.bytes.saturating_add(incoming) > max_bytes {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                return;
            };
            self.bytes = self.bytes.saturating_sub(evicted.len());
        }

        self.entries.put(key, body);
        self.bytes = self.bytes.saturating_add(incoming);
    }
}

pub struct RawFrameCache {
    entries: LruCache<RawFrameCacheKey, (Bytes, RawFrameMetadata)>,
    bytes: usize,
}

impl RawFrameCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: LruCache::new(
                NonZeroUsize::new(capacity).expect("non-zero raw cache capacity"),
            ),
            bytes: 0,
        }
    }

    fn get(&mut self, key: &RawFrameCacheKey) -> Option<(Bytes, RawFrameMetadata)> {
        self.entries.get(key).cloned()
    }

    fn insert_with_budget(
        &mut self,
        key: RawFrameCacheKey,
        body: Bytes,
        metadata: RawFrameMetadata,
        max_bytes: usize,
    ) {
        let incoming = body.len();
        if incoming > max_bytes {
            return;
        }

        if let Some((existing, _)) = self.entries.pop(&key) {
            self.bytes = self.bytes.saturating_sub(existing.len());
        }

        while self.bytes.saturating_add(incoming) > max_bytes {
            let Some((_, (evicted, _))) = self.entries.pop_lru() else {
                return;
            };
            self.bytes = self.bytes.saturating_sub(evicted.len());
        }

        self.entries.put(key, (body, metadata));
        self.bytes = self.bytes.saturating_add(incoming);
    }
}

pub fn new_cache() -> Arc<Mutex<FrameCache>> {
    Arc::new(Mutex::new(FrameCache::new(CACHE_CAPACITY)))
}

pub fn new_raw_cache() -> Arc<Mutex<RawFrameCache>> {
    Arc::new(Mutex::new(RawFrameCache::new(RAW_CACHE_CAPACITY)))
}

#[derive(Debug, Clone)]
pub struct RawFrameRequest {
    pub frame: u32,
}

#[derive(Debug, Clone)]
pub struct RawFrameResponse {
    pub body: Bytes,
    pub metadata: RawFrameMetadata,
    pub cache_hit: bool,
}

pub async fn load_raw_frame(
    file: FileEntry,
    cache: Arc<Mutex<RawFrameCache>>,
    request: RawFrameRequest,
) -> PixelResult<RawFrameResponse> {
    if !file.has_pixels {
        return Err(PixelError::NoPixelData);
    }
    if request.frame >= file.frame_count {
        return Err(PixelError::FrameOutOfRange);
    }

    let syntax_class = classify_transfer_syntax(&file.transfer_syntax_uid);
    if matches!(
        syntax_class,
        TransferSyntaxClass::JpegLs | TransferSyntaxClass::Rle | TransferSyntaxClass::Unsupported
    ) {
        return Err(PixelError::UnsupportedTransferSyntax(
            file.transfer_syntax_uid.clone(),
        ));
    }

    let key = RawFrameCacheKey {
        file_index: file.index,
        frame: request.frame,
    };

    if let Ok(mut lock) = cache.lock() {
        if let Some((bytes, meta)) = lock.get(&key) {
            return Ok(RawFrameResponse {
                body: bytes,
                metadata: meta,
                cache_hit: true,
            });
        }
    }

    let (body, metadata) = match syntax_class {
        TransferSyntaxClass::Jpeg => read_raw_jpeg_samples(file.clone(), request.frame)
            .await
            .map_err(PixelError::raw_decode)?,
        TransferSyntaxClass::JpegLossless => {
            decode_raw_jpeg_lossless(file.clone(), request.frame).await?
        }
        TransferSyntaxClass::Jpeg2000 => {
            decode_raw_jp2_samples(file.clone(), request.frame).await?
        }
        TransferSyntaxClass::Uncompressed => read_raw_uncompressed(file.clone(), request.frame)
            .await
            .map_err(PixelError::raw_decode)?,
        _ => unreachable!("non-raw syntaxes filtered above"),
    };

    if let Ok(mut lock) = cache.lock() {
        lock.insert_with_budget(key, body.clone(), metadata.clone(), RAW_CACHE_MAX_BYTES);
    }

    Ok(RawFrameResponse {
        body,
        metadata,
        cache_hit: false,
    })
}

#[derive(Debug, Clone)]
pub struct FrameRequest {
    pub frame: u32,
    pub window_center: Option<f64>,
    pub window_width: Option<f64>,
    pub window_mode: WindowMode,
}

#[derive(Debug, Clone)]
pub struct FrameResponse {
    pub body: Bytes,
    pub content_type: &'static str,
    pub cache_hit: bool,
}

pub async fn load_frame(
    file: FileEntry,
    cache: Arc<Mutex<FrameCache>>,
    request: FrameRequest,
) -> PixelResult<FrameResponse> {
    if !file.has_pixels {
        return Err(PixelError::NoPixelData);
    }
    if request.frame >= file.frame_count {
        return Err(PixelError::FrameOutOfRange);
    }

    let window = WindowRequest::new(
        request.window_center,
        request.window_width,
        request.window_mode,
    )
    .map_err(|error| PixelError::InvalidWindow(error.to_string()))?;
    let syntax_class = classify_transfer_syntax(&file.transfer_syntax_uid);
    let key = FrameCacheKey::new(
        file.index,
        request.frame,
        window.center(),
        window.width(),
        window.mode(),
    );

    if let Ok(mut lock) = cache.lock() {
        if let Some(bytes) = lock.get(&key) {
            return Ok(FrameResponse {
                body: bytes,
                content_type: "image/png",
                cache_hit: true,
            });
        }
    }

    let (body, content_type) = match syntax_class {
        TransferSyntaxClass::Jpeg => (
            decode_compressed_frame_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::JpegLossless => (
            decode_compressed_frame_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::Jpeg2000 => (
            decode_jp2_fragment_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::Uncompressed => (
            decode_uncompressed_to_png(
                file.clone(),
                request.frame,
                window.center(),
                window.width(),
                window.mode(),
            )
            .await
            .map_err(PixelError::frame_decode)?,
            "image/png",
        ),
        TransferSyntaxClass::JpegLs
        | TransferSyntaxClass::Rle
        | TransferSyntaxClass::Unsupported => {
            return Err(PixelError::UnsupportedTransferSyntax(
                file.transfer_syntax_uid.clone(),
            ));
        }
    };

    if let Ok(mut lock) = cache.lock() {
        lock.insert_with_budget(key, body.clone(), FRAME_CACHE_MAX_BYTES);
    }

    Ok(FrameResponse {
        body,
        content_type,
        cache_hit: false,
    })
}

fn read_encapsulated_fragment_blocking(path: &PathBuf, frame: u32) -> Result<Bytes> {
    let mut collector = DicomCollector::open_file(path).with_context(|| {
        format!(
            "failed to open DICOM for collector access: {}",
            path.display()
        )
    })?;

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
    file: FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    task::spawn_blocking(move || {
        decode_jp2_fragment_to_png_blocking(&file, frame, requested_wc, requested_ww, window_mode)
    })
    .await
    .context("jp2 fragment decode task failed")?
}

fn decode_jp2_fragment_to_png_blocking(
    file: &FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    let fragment = read_encapsulated_fragment_blocking(&file.path, frame)?;

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
        let resolved_window = resolve_window_with_mode(
            window_mode,
            requested_wc,
            requested_ww,
            file.default_window,
            &raw_samples,
        )
        .ok_or_else(|| anyhow!("JP2 decode failed: could not resolve window"))?;
        let mut windowed = apply_window(
            &raw_samples,
            resolved_window.center,
            resolved_window.width.max(1.0),
        );
        apply_monochrome1_inversion(&mut windowed, &file.photometric_interpretation);
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
            let interleaved: Vec<u8> = r
                .zip(g)
                .zip(b)
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
            let interleaved: Vec<u16> = r
                .zip(g)
                .zip(b)
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

async fn decode_compressed_frame_to_png(
    file: FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    task::spawn_blocking(move || {
        decode_compressed_frame_to_png_blocking(
            &file,
            frame,
            requested_wc,
            requested_ww,
            window_mode,
        )
    })
    .await
    .context("compressed decode task failed")?
}

fn decode_compressed_frame_to_png_blocking(
    file: &FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    let obj = open_file(&file.path).with_context(|| {
        format!(
            "failed to open DICOM for decode fallback: {}",
            file.path.display()
        )
    })?;
    let decoded = obj.decode_pixel_data().with_context(|| {
        format!(
            "unsupported transfer syntax: {}",
            obj.meta().transfer_syntax()
        )
    })?;
    let (samples, rows, columns) = decoded_luminance_samples(file, &decoded, frame)?;
    encode_windowed_luminance_png(
        file,
        samples,
        rows,
        columns,
        requested_wc,
        requested_ww,
        window_mode,
    )
}

fn decoded_luminance_samples(
    file: &FileEntry,
    decoded: &dicom_pixeldata::DecodedPixelData<'_>,
    frame: u32,
) -> Result<(Vec<f64>, u32, u32)> {
    let bits_allocated = decoded.bits_allocated() as u32;
    let signed = file.pixel_representation == 1;
    let samples_per_pixel = decoded.samples_per_pixel().max(1) as usize;
    let raw_samples = match bits_allocated {
        8 => decoded
            .frame_data(frame)?
            .iter()
            .map(|value| {
                if signed {
                    (*value as i8) as f64
                } else {
                    *value as f64
                }
            })
            .collect::<Vec<_>>(),
        16 => decoded
            .frame_data_ow(frame)?
            .into_iter()
            .map(|value| {
                if signed {
                    (value as i16) as f64
                } else {
                    value as f64
                }
            })
            .collect::<Vec<_>>(),
        _ => {
            return Err(anyhow!(
                "compressed decode failed: unsupported BitsAllocated {bits_allocated}"
            ));
        }
    };

    let luminance_samples = if samples_per_pixel == 1 {
        raw_samples
    } else {
        raw_samples
            .chunks(samples_per_pixel)
            .map(|chunk| chunk[0])
            .collect::<Vec<_>>()
    };

    let rescaled = luminance_samples
        .into_iter()
        .map(|value| value * file.rescale_slope + file.rescale_intercept)
        .collect::<Vec<_>>();

    Ok((rescaled, decoded.rows(), decoded.columns()))
}

fn encode_windowed_luminance_png(
    file: &FileEntry,
    rescaled: Vec<f64>,
    rows: u32,
    columns: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    let resolved_window = resolve_window_with_mode(
        window_mode,
        requested_wc,
        requested_ww,
        file.default_window,
        &rescaled,
    )
    .ok_or_else(|| anyhow!("compressed decode failed: could not resolve window"))?;
    let mut windowed = apply_window(
        &rescaled,
        resolved_window.center,
        resolved_window.width.max(1.0),
    );
    apply_monochrome1_inversion(&mut windowed, &file.photometric_interpretation);

    let image = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(columns, rows, windowed)
        .ok_or_else(|| anyhow!("compressed decode failed: windowed buffer size mismatch"))?;
    let mut buffer = Cursor::new(Vec::<u8>::new());
    image::DynamicImage::ImageLuma8(image)
        .write_to(&mut buffer, ImageFormat::Png)
        .context("compressed decode failed: png encoding failed")?;
    Ok(Bytes::from(buffer.into_inner()))
}

async fn decode_uncompressed_to_png(
    file: FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    task::spawn_blocking(move || {
        decode_uncompressed_to_png_blocking(&file, frame, requested_wc, requested_ww, window_mode)
    })
    .await
    .context("uncompressed decode task failed")?
}

fn decode_uncompressed_to_png_blocking(
    file: &FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> Result<Bytes> {
    let object = open_file(&file.path).with_context(|| {
        format!(
            "failed to open DICOM for uncompressed decode: {}",
            file.path.display()
        )
    })?;

    let rows = file.rows;
    let columns = file.columns;
    let samples_per_pixel = file.samples_per_pixel.max(1);
    let bits_allocated = file.bits_allocated;
    let bytes_per_sample = (bits_allocated / 8) as usize;
    if rows == 0 || columns == 0 || bytes_per_sample == 0 {
        return Err(anyhow!("frame decode failed: invalid image geometry"));
    }

    let frame_size =
        rows as usize * columns as usize * samples_per_pixel as usize * bytes_per_sample;
    let offset = frame as usize * frame_size;

    let pixel_bytes = object
        .element_by_name("PixelData")
        .context("frame decode failed: missing PixelData")?
        .to_bytes()
        .context("frame decode failed: pixel bytes unavailable")?;

    if offset + frame_size > pixel_bytes.len() {
        return Err(anyhow!("frame out of range"));
    }

    let frame_slice = &pixel_bytes[offset..offset + frame_size];
    let signed = file.pixel_representation == 1;
    // dicom-object normalizes primitive pixel bytes to host order for native pixel data.
    // Decode from the normalized byte representation directly.
    let raw_samples = decode_numeric_samples(frame_slice, bits_allocated, signed, false)?;
    let rescaled: Vec<f64> = raw_samples
        .into_iter()
        .map(|value| value * file.rescale_slope + file.rescale_intercept)
        .collect();

    let luminance_samples = if samples_per_pixel > 1 {
        rescaled
            .chunks(samples_per_pixel as usize)
            .map(|chunk| chunk[0])
            .collect::<Vec<_>>()
    } else {
        rescaled
    };

    let resolved_window = resolve_window_with_mode(
        window_mode,
        requested_wc,
        requested_ww,
        file.default_window,
        &luminance_samples,
    )
    .ok_or_else(|| anyhow!("frame decode failed: could not resolve window"))?;
    let mut windowed = apply_window(
        &luminance_samples,
        resolved_window.center,
        resolved_window.width.max(1.0),
    );
    apply_monochrome1_inversion(&mut windowed, &file.photometric_interpretation);

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
        (8, true) => Ok(frame_slice
            .iter()
            .map(|value| (*value as i8) as f64)
            .collect()),
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

async fn read_raw_uncompressed(file: FileEntry, frame: u32) -> Result<(Bytes, RawFrameMetadata)> {
    task::spawn_blocking(move || read_raw_uncompressed_blocking(&file, frame))
        .await
        .context("raw uncompressed read task failed")?
}

fn read_raw_uncompressed_blocking(
    file: &FileEntry,
    frame: u32,
) -> Result<(Bytes, RawFrameMetadata)> {
    let object = open_file(&file.path).with_context(|| {
        format!(
            "failed to open DICOM for raw uncompressed read: {}",
            file.path.display()
        )
    })?;

    let rows = file.rows;
    let columns = file.columns;
    let samples_per_pixel = file.samples_per_pixel.max(1);
    let bits_allocated = file.bits_allocated;
    let bytes_per_sample = (bits_allocated / 8).max(1) as usize;
    if rows == 0 || columns == 0 {
        return Err(anyhow!("frame decode failed: invalid image geometry"));
    }

    let frame_size =
        rows as usize * columns as usize * samples_per_pixel as usize * bytes_per_sample;
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

    let metadata = file.raw_metadata(rows, columns, bits_allocated, samples_per_pixel);
    Ok((Bytes::from(frame_bytes), metadata))
}

async fn read_raw_jpeg_samples(file: FileEntry, frame: u32) -> Result<(Bytes, RawFrameMetadata)> {
    task::spawn_blocking(move || read_raw_jpeg_samples_blocking(&file, frame))
        .await
        .context("raw JPEG sample read task failed")?
}

fn read_raw_jpeg_samples_blocking(
    file: &FileEntry,
    frame: u32,
) -> Result<(Bytes, RawFrameMetadata)> {
    let fragment = read_encapsulated_fragment_blocking(&file.path, frame)?;
    // Decode JPEG to 8-bit grayscale samples. Tolerates Baseline and Extended JPEG.
    let img = image::load_from_memory(&fragment)
        .context("JPEG decode failed for raw samples")?
        .to_luma8();
    let (columns, rows) = (img.width(), img.height());
    let samples = img.into_raw();

    let metadata = file.normalized_grayscale_u8_metadata(rows, columns);
    Ok((Bytes::from(samples), metadata))
}

async fn decode_raw_jpeg_lossless(
    file: FileEntry,
    frame: u32,
) -> PixelResult<(Bytes, RawFrameMetadata)> {
    task::spawn_blocking(move || decode_raw_jpeg_lossless_blocking(&file, frame))
        .await
        .map_err(|error| {
            PixelError::raw_decode(anyhow!("raw JPEG Lossless decode task failed: {error}"))
        })?
}

fn decode_raw_jpeg_lossless_blocking(
    file: &FileEntry,
    frame: u32,
) -> PixelResult<(Bytes, RawFrameMetadata)> {
    let obj = open_file(&file.path)
        .with_context(|| {
            format!(
                "failed to open DICOM for raw JPEG Lossless decode: {}",
                file.path.display()
            )
        })
        .map_err(PixelError::raw_decode)?;

    let decoded = obj
        .decode_pixel_data()
        .with_context(|| {
            format!(
                "unsupported transfer syntax: {}",
                obj.meta().transfer_syntax()
            )
        })
        .map_err(PixelError::raw_decode)?;
    if decoded.samples_per_pixel() != 1 {
        return Err(PixelError::UnsupportedLayout(format!(
            "raw JPEG Lossless requires one sample per pixel, decoded {}",
            decoded.samples_per_pixel()
        )));
    }

    let bits_allocated = decoded.bits_allocated() as u32;
    let sample_bytes = match bits_allocated {
        8 => Bytes::copy_from_slice(
            decoded
                .frame_data(frame)
                .map_err(anyhow::Error::from)
                .map_err(PixelError::raw_decode)?,
        ),
        16 => {
            let bytes = decoded
                .frame_data_ow(frame)
                .map_err(anyhow::Error::from)
                .map_err(PixelError::raw_decode)?
                .into_iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            Bytes::from(bytes)
        }
        _ => {
            return Err(PixelError::UnsupportedLayout(format!(
                "raw JPEG Lossless does not support BitsAllocated {bits_allocated}"
            )));
        }
    };

    let metadata = file.raw_metadata(decoded.rows(), decoded.columns(), bits_allocated, 1);
    Ok((sample_bytes, metadata))
}

async fn decode_raw_jp2_samples(
    file: FileEntry,
    frame: u32,
) -> PixelResult<(Bytes, RawFrameMetadata)> {
    task::spawn_blocking(move || decode_raw_jp2_samples_blocking(&file, frame))
        .await
        .map_err(|error| PixelError::raw_decode(anyhow!("raw JP2 decode task failed: {error}")))?
}

fn decode_raw_jp2_samples_blocking(
    file: &FileEntry,
    frame: u32,
) -> PixelResult<(Bytes, RawFrameMetadata)> {
    let fragment =
        read_encapsulated_fragment_blocking(&file.path, frame).map_err(PixelError::raw_decode)?;

    let jp2_image = jpeg2k::Image::from_bytes(&fragment)
        .map_err(anyhow::Error::from)
        .context("failed to decode JP2 fragment for raw samples")
        .map_err(PixelError::raw_decode)?;

    let comps = jp2_image.components();
    if comps.is_empty() {
        return Err(PixelError::raw_decode(anyhow!(
            "JP2 image has no components"
        )));
    }

    // Only grayscale (single component) is supported for the raw path.
    if comps.len() != 1 {
        return Err(PixelError::UnsupportedLayout(format!(
            "raw JPEG 2000 requires one component, decoded {}",
            comps.len()
        )));
    }

    let width = comps[0].width();
    let height = comps[0].height();
    let precision = comps[0].precision();

    let (sample_bytes, bits_allocated) = if precision <= 8 {
        let samples: Vec<u8> = comps[0].data_u8().collect();
        (Bytes::from(samples), 8u32)
    } else {
        // Normalize to u16 LE for any precision 9-16.
        let bytes: Vec<u8> = comps[0].data_u16().flat_map(|v| v.to_le_bytes()).collect();
        (Bytes::from(bytes), 16u32)
    };

    let metadata = file.raw_metadata(height, width, bits_allocated, 1);
    Ok((sample_bytes, metadata))
}

pub fn classify_transfer_syntax(uid: &str) -> TransferSyntaxClass {
    match uid {
        // Browser-renderable lossy JPEG: Baseline, Extended
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => TransferSyntaxClass::Jpeg,
        // JPEG Lossless: browsers cannot decode — must be decoded server-side
        "1.2.840.10008.1.2.4.57" | "1.2.840.10008.1.2.4.70" => TransferSyntaxClass::JpegLossless,
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
    let p99_idx =
        (((values.len() as f64) * 0.99).ceil() as usize).min(values.len().saturating_sub(1));
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

fn is_monochrome1(photometric_interpretation: &str) -> bool {
    photometric_interpretation
        .trim()
        .eq_ignore_ascii_case("MONOCHROME1")
}

fn apply_monochrome1_inversion(samples: &mut [u8], photometric_interpretation: &str) {
    if is_monochrome1(photometric_interpretation) {
        for sample in samples {
            *sample = 255_u8.saturating_sub(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_key(frame: u32) -> FrameCacheKey {
        FrameCacheKey::new(0, frame, None, None, WindowMode::Default)
    }

    fn raw_key(frame: u32) -> RawFrameCacheKey {
        RawFrameCacheKey {
            file_index: 0,
            frame,
        }
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

    fn frame_cache_contains(cache: &FrameCache, key: &FrameCacheKey) -> bool {
        cache
            .entries
            .iter()
            .any(|(cached_key, _)| cached_key == key)
    }

    fn raw_cache_contains(cache: &RawFrameCache, key: &RawFrameCacheKey) -> bool {
        cache
            .entries
            .iter()
            .any(|(cached_key, _)| cached_key == key)
    }

    #[test]
    fn frame_cache_budget_evicts_lru_entries() {
        let mut cache = FrameCache::new(4);
        let key0 = frame_key(0);
        let key1 = frame_key(1);
        let key2 = frame_key(2);

        cache.insert_with_budget(key0.clone(), Bytes::from(vec![0_u8; 4]), 8);
        cache.insert_with_budget(key1.clone(), Bytes::from(vec![1_u8; 4]), 8);
        cache.insert_with_budget(key2.clone(), Bytes::from(vec![2_u8; 4]), 8);

        assert!(
            !frame_cache_contains(&cache, &key0),
            "least-recently-used entry should be evicted"
        );
        assert!(
            frame_cache_contains(&cache, &key1),
            "second entry should still be cached"
        );
        assert!(
            frame_cache_contains(&cache, &key2),
            "new entry should be cached"
        );
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn frame_cache_budget_skips_oversized_entries() {
        let mut cache = FrameCache::new(4);
        let key0 = frame_key(0);

        cache.insert_with_budget(key0.clone(), Bytes::from(vec![0_u8; 9]), 8);

        assert!(
            !frame_cache_contains(&cache, &key0),
            "oversized entry should be skipped"
        );
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn raw_cache_budget_evicts_lru_entries() {
        let mut cache = RawFrameCache::new(4);
        let key0 = raw_key(0);
        let key1 = raw_key(1);
        let key2 = raw_key(2);

        cache.insert_with_budget(key0.clone(), Bytes::from(vec![0_u8; 4]), raw_meta(), 8);
        cache.insert_with_budget(key1.clone(), Bytes::from(vec![1_u8; 4]), raw_meta(), 8);
        cache.insert_with_budget(key2.clone(), Bytes::from(vec![2_u8; 4]), raw_meta(), 8);

        assert!(
            !raw_cache_contains(&cache, &key0),
            "least-recently-used raw entry should be evicted"
        );
        assert!(
            raw_cache_contains(&cache, &key1),
            "second raw entry should still be cached"
        );
        assert!(
            raw_cache_contains(&cache, &key2),
            "new raw entry should be cached"
        );
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn frame_cache_replacement_updates_tracked_bytes() {
        let mut cache = FrameCache::new(4);
        let key = frame_key(0);

        cache.insert_with_budget(key.clone(), Bytes::from(vec![0_u8; 6]), 8);
        cache.insert_with_budget(key.clone(), Bytes::from(vec![1_u8; 3]), 8);

        assert!(frame_cache_contains(&cache, &key));
        assert_eq!(cache.bytes, 3);
    }
}
