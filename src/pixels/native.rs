use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::{FileEntry, NativePixelDataKind};
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::open_file;
use image::{ImageBuffer, ImageFormat, Luma};
use std::io::Cursor;
use tokio::task;

use super::color::{encode_rgb8_png, rgb8_interleaved, ybr_full_to_rgb8};
use super::native_layout::{native_pixel_element_tag, NativeByteOrder, NativeFrameLayout};
use super::palette::palette_indices_to_rgb8;
use super::render::apply_monochrome1_inversion;
use super::window::{apply_modality_transform, apply_window, resolve_window_with_mode};

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
    let layout = native_frame_layout(file);
    let pixel_bytes = read_native_pixel_bytes(&object, file)?;
    let frame_bytes = layout
        .extract_display_frame(&pixel_bytes, frame)
        .context("frame decode failed: invalid native frame layout")?;
    let pixel_count = usize::try_from(rows)
        .ok()
        .and_then(|rows| {
            usize::try_from(columns)
                .ok()
                .and_then(|columns| rows.checked_mul(columns))
        })
        .ok_or_else(|| anyhow!("frame decode failed: invalid image geometry"))?;
    let photometric = file.photometric_interpretation.trim().to_ascii_uppercase();
    if bits_allocated == 8 {
        let rgb = match (samples_per_pixel, photometric.as_str()) {
            (3, "RGB") => Some(rgb8_interleaved(&frame_bytes, pixel_count, 0)?),
            (3, "YBR_FULL" | "YBR_FULL_422") => {
                Some(ybr_full_to_rgb8(&frame_bytes, pixel_count, 0)?)
            }
            (1, "PALETTE COLOR") => Some(palette_indices_to_rgb8(
                &file.path,
                &frame_bytes,
                bits_allocated,
            )?),
            _ => None,
        };
        if let Some(rgb) = rgb {
            return encode_rgb8_png(rgb, columns, rows)
                .context("frame decode failed: color PNG encoding failed");
        }
    }
    if samples_per_pixel != 1 || !matches!(photometric.as_str(), "MONOCHROME1" | "MONOCHROME2") {
        return Err(anyhow!(
            "frame decode failed: unsupported native layout SamplesPerPixel {samples_per_pixel}, PhotometricInterpretation {}",
            file.photometric_interpretation
        ));
    }

    let signed = file.pixel_representation == 1;
    // dicom-object normalizes primitive pixel bytes to host order for native pixel data.
    // Decode from the normalized byte representation directly.
    let raw_samples = decode_numeric_samples(
        &frame_bytes,
        bits_allocated,
        signed,
        false,
        native_pixel_data_kind(file),
    )?;
    let rescaled = apply_modality_transform(
        &raw_samples,
        file.series_metadata.native_pixel.modality_lut.as_ref(),
        file.rescale_slope,
        file.rescale_intercept,
    );

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

fn native_frame_layout(file: &FileEntry) -> NativeFrameLayout<'_> {
    NativeFrameLayout {
        rows: file.rows,
        columns: file.columns,
        samples_per_pixel: file.samples_per_pixel.max(1),
        bits_allocated: file.bits_allocated,
        planar_configuration: file.series_metadata.native_pixel.planar_configuration,
        photometric_interpretation: &file.photometric_interpretation,
        // dicom-object normalizes native primitive values to host order. The
        // supported release hosts are little-endian, matching the raw API.
        byte_order: NativeByteOrder::LittleEndian,
    }
}

fn read_native_pixel_bytes(
    object: &dicom_object::DefaultDicomObject,
    file: &FileEntry,
) -> Result<Vec<u8>> {
    let kind = native_pixel_data_kind(file);
    object
        .element(native_pixel_element_tag(kind))
        .context("frame decode failed: missing native pixel data element")?
        .to_bytes()
        .context("frame decode failed: pixel bytes unavailable")
        .map(|bytes| bytes.into_owned())
}

fn native_pixel_data_kind(file: &FileEntry) -> NativePixelDataKind {
    file.series_metadata
        .native_pixel
        .pixel_data_kind
        .unwrap_or(NativePixelDataKind::Integer)
}

fn decode_numeric_samples(
    frame_slice: &[u8],
    bits_allocated: u32,
    signed: bool,
    big_endian: bool,
    kind: NativePixelDataKind,
) -> Result<Vec<f64>> {
    match (kind, bits_allocated, signed) {
        (NativePixelDataKind::Float32, 32, _) => Ok(frame_slice
            .chunks_exact(4)
            .map(|chunk| {
                let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                (if big_endian {
                    f32::from_be_bytes(bytes)
                } else {
                    f32::from_le_bytes(bytes)
                }) as f64
            })
            .collect()),
        (NativePixelDataKind::Float64, 64, _) => Ok(frame_slice
            .chunks_exact(8)
            .map(|chunk| {
                let bytes = [
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ];
                if big_endian {
                    f64::from_be_bytes(bytes)
                } else {
                    f64::from_le_bytes(bytes)
                }
            })
            .collect()),
        (NativePixelDataKind::Integer, 1, false) => {
            Ok(frame_slice.iter().map(|value| f64::from(*value)).collect())
        }
        (NativePixelDataKind::Integer, 8, false) => {
            Ok(frame_slice.iter().map(|value| *value as f64).collect())
        }
        (NativePixelDataKind::Integer, 8, true) => Ok(frame_slice
            .iter()
            .map(|value| (*value as i8) as f64)
            .collect()),
        (NativePixelDataKind::Integer, 16, false) => {
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
        (NativePixelDataKind::Integer, 16, true) => {
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
        (NativePixelDataKind::Integer, 32, false) => Ok(frame_slice
            .chunks_exact(4)
            .map(|chunk| {
                let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                (if big_endian {
                    u32::from_be_bytes(bytes)
                } else {
                    u32::from_le_bytes(bytes)
                }) as f64
            })
            .collect()),
        (NativePixelDataKind::Integer, 32, true) => Ok(frame_slice
            .chunks_exact(4)
            .map(|chunk| {
                let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                (if big_endian {
                    i32::from_be_bytes(bytes)
                } else {
                    i32::from_le_bytes(bytes)
                }) as f64
            })
            .collect()),
        _ => Err(anyhow!(
            "frame decode failed: unsupported native sample kind {kind:?} with BitsAllocated {bits_allocated}"
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
    let pixel_bytes = read_native_pixel_bytes(&object, file)?;
    let frame_bytes = native_frame_layout(file)
        .extract_raw_frame(&pixel_bytes, frame)
        .context("frame decode failed: invalid native frame layout")?;

    let metadata = file.raw_metadata(rows, columns, bits_allocated, samples_per_pixel);
    Ok((Bytes::from(frame_bytes), metadata))
}

#[cfg(test)]
mod tests {
    use super::decode_numeric_samples;
    use crate::types::NativePixelDataKind;

    #[test]
    fn decodes_one_bit_and_32_bit_integer_samples() {
        assert_eq!(
            decode_numeric_samples(&[1, 0, 1], 1, false, false, NativePixelDataKind::Integer,)
                .unwrap(),
            [1.0, 0.0, 1.0]
        );
        let unsigned = [0_u32, 65_535, 2_147_483_648, u32::MAX]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_numeric_samples(&unsigned, 32, false, false, NativePixelDataKind::Integer,)
                .unwrap(),
            [0.0, 65_535.0, 2_147_483_648.0, 4_294_967_295.0]
        );
    }

    #[test]
    fn decodes_float_and_double_float_samples() {
        let floats = [-256.0_f32, 0.5, 512.25]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_numeric_samples(&floats, 32, false, false, NativePixelDataKind::Float32,)
                .unwrap(),
            [-256.0, 0.5, 512.25]
        );
        let doubles = [-256.0_f64, 0.5, 511.750_000_001_862_65]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_numeric_samples(&doubles, 64, false, false, NativePixelDataKind::Float64,)
                .unwrap(),
            [-256.0, 0.5, 511.750_000_001_862_65]
        );
    }
}
