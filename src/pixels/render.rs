use crate::api::contracts::WindowMode;
use crate::types::FileEntry;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use image::{ImageBuffer, ImageFormat, Luma};
use std::io::Cursor;

use super::window::{apply_window, resolve_window_with_mode};

pub(crate) fn encode_windowed_luminance_png(
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

fn is_monochrome1(photometric_interpretation: &str) -> bool {
    photometric_interpretation
        .trim()
        .eq_ignore_ascii_case("MONOCHROME1")
}

pub(crate) fn apply_monochrome1_inversion(samples: &mut [u8], photometric_interpretation: &str) {
    if is_monochrome1(photometric_interpretation) {
        for sample in samples {
            *sample = 255_u8.saturating_sub(*sample);
        }
    }
}
