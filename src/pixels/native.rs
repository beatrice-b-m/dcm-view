use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::FileEntry;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::open_file;
use image::{ImageBuffer, ImageFormat, Luma};
use std::io::Cursor;
use tokio::task;

use super::render::apply_monochrome1_inversion;
use super::window::{apply_window, resolve_window_with_mode};

pub(crate) async fn decode_uncompressed_to_png(
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

pub(crate) async fn read_raw_uncompressed(
    file: FileEntry,
    frame: u32,
) -> Result<(Bytes, RawFrameMetadata)> {
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
