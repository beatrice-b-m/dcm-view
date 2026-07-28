use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::FileEntry;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use image::{ImageBuffer, ImageFormat, Luma, Rgb};
use std::io::Cursor;
use tokio::task;

use super::encapsulated::read_encapsulated_fragment_blocking;
use super::error::{PixelError, PixelResult};
use super::render::apply_monochrome1_inversion;
use super::window::{apply_window, resolve_window_with_mode};

pub(crate) async fn decode_jp2_fragment_to_png(
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

pub(crate) async fn decode_raw_jp2_samples(
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
