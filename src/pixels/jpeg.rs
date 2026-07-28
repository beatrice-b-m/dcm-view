use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::FileEntry;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::open_file;
use dicom_pixeldata::PixelDecoder;
use tokio::task;

use super::encapsulated::read_encapsulated_fragment_blocking;
use super::error::{PixelError, PixelResult};
use super::render::encode_windowed_luminance_png;

pub(crate) async fn decode_compressed_frame_to_png(
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

pub(crate) async fn read_raw_jpeg_samples(
    file: FileEntry,
    frame: u32,
) -> Result<(Bytes, RawFrameMetadata)> {
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

pub(crate) async fn decode_raw_jpeg_lossless(
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
