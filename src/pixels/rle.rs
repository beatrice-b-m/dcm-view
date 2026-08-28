use crate::api::contracts::{RawFrameMetadata, WindowMode};
use crate::types::FileEntry;
use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::open_file;
use thiserror::Error;
use tokio::task;

use super::color::{encode_rgb8_png_with_icc, rgb8_interleaved, ybr_full_to_rgb8};
use super::encapsulated::read_encapsulated_fragment_blocking;
use super::error::{PixelError, PixelResult};
use super::icc::select_icc_profile;
use super::render::encode_windowed_luminance_png;

const RLE_HEADER_LEN: usize = 64;
const RLE_MAX_SEGMENTS: usize = 15;

#[derive(Debug, Error, PartialEq, Eq)]
enum RleDecodeError {
    #[error("RLE frame is shorter than the 64-byte header")]
    HeaderTooShort,
    #[error("RLE header declares invalid segment count {0}; expected 1-15")]
    InvalidSegmentCount(u32),
    #[error("RLE pixel layout has invalid zero-valued geometry or SamplesPerPixel")]
    InvalidLayout,
    #[error("RLE BitsAllocated {0} is unsupported; expected 8 or 16")]
    UnsupportedBitsAllocated(u32),
    #[error("RLE header declares {actual} segments, but the pixel layout requires {expected}")]
    SegmentCountMismatch { actual: usize, expected: usize },
    #[error("RLE segment {segment} offset {offset} is invalid for fragment length {length}")]
    InvalidSegmentOffset {
        segment: usize,
        offset: usize,
        length: usize,
    },
    #[error("RLE segment offsets are not strictly increasing at segment {segment}")]
    NonIncreasingSegmentOffsets { segment: usize },
    #[error("RLE header contains a non-zero unused segment offset at slot {slot}")]
    NonZeroUnusedOffset { slot: usize },
    #[error("RLE PackBits literal run is truncated")]
    TruncatedLiteralRun,
    #[error("RLE PackBits repeat run is truncated")]
    TruncatedRepeatRun,
    #[error("RLE PackBits run exceeds the expected decoded segment length {expected}")]
    SegmentOutputOverflow { expected: usize },
    #[error("RLE PackBits segment decoded to {actual} bytes, expected {expected}")]
    SegmentOutputLength { actual: usize, expected: usize },
    #[error("RLE pixel layout size overflowed")]
    LayoutSizeOverflow,
}

pub(crate) async fn decode_rle_to_png(
    file: FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> PixelResult<Bytes> {
    task::spawn_blocking(move || {
        decode_rle_to_png_blocking(&file, frame, requested_wc, requested_ww, window_mode)
    })
    .await
    .map_err(|error| PixelError::frame_decode(anyhow!("RLE decode task failed: {error}")))?
}

fn decode_rle_to_png_blocking(
    file: &FileEntry,
    frame: u32,
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    window_mode: WindowMode,
) -> PixelResult<Bytes> {
    let decoded = read_and_decode_frame(file, frame).map_err(PixelError::frame_decode)?;
    let photometric = file.photometric_interpretation.trim().to_ascii_uppercase();

    match (file.samples_per_pixel, photometric.as_str()) {
        (1, "MONOCHROME1" | "MONOCHROME2") => {
            let samples = decode_monochrome_samples(
                &decoded,
                file.bits_allocated,
                file.pixel_representation,
            )?;
            let rescaled = samples
                .into_iter()
                .map(|value| value * file.rescale_slope + file.rescale_intercept)
                .collect();
            encode_windowed_luminance_png(
                file,
                rescaled,
                file.rows,
                file.columns,
                requested_wc,
                requested_ww,
                window_mode,
            )
            .map_err(PixelError::frame_decode)
        }
        (3, "RGB" | "YBR_FULL") => encode_rgb_png(
            file,
            normalize_color_for_display(
                &decoded,
                file.rows,
                file.columns,
                file.series_metadata
                    .native_pixel
                    .planar_configuration
                    .unwrap_or(0),
                &photometric,
            )?,
            read_icc_profile(file)?,
        ),
        (1, "PALETTE COLOR") => encode_palette_png(file, &decoded),
        _ => Err(PixelError::UnsupportedLayout(format!(
            "RLE display does not support SamplesPerPixel {} with PhotometricInterpretation {}",
            file.samples_per_pixel, file.photometric_interpretation
        ))),
    }
}

pub(crate) async fn decode_raw_rle(
    file: FileEntry,
    frame: u32,
) -> PixelResult<(Bytes, RawFrameMetadata)> {
    task::spawn_blocking(move || {
        let decoded = read_and_decode_frame(&file, frame).map_err(PixelError::raw_decode)?;
        let metadata = file.raw_metadata(
            file.rows,
            file.columns,
            file.bits_allocated,
            file.samples_per_pixel,
        );
        Ok((Bytes::from(decoded), metadata))
    })
    .await
    .map_err(|error| PixelError::raw_decode(anyhow!("raw RLE decode task failed: {error}")))?
}

fn read_and_decode_frame(file: &FileEntry, frame: u32) -> Result<Vec<u8>> {
    let fragment = read_encapsulated_fragment_blocking(&file.path, frame)?;
    decode_rle_frame(
        fragment.as_ref(),
        file.rows,
        file.columns,
        file.samples_per_pixel,
        file.bits_allocated,
    )
    .map_err(anyhow::Error::from)
}

fn decode_rle_frame(
    fragment: &[u8],
    rows: u32,
    columns: u32,
    samples_per_pixel: u32,
    bits_allocated: u32,
) -> std::result::Result<Vec<u8>, RleDecodeError> {
    if fragment.len() < RLE_HEADER_LEN {
        return Err(RleDecodeError::HeaderTooShort);
    }
    let bytes_per_sample = match bits_allocated {
        8 => 1_usize,
        16 => 2_usize,
        _ => return Err(RleDecodeError::UnsupportedBitsAllocated(bits_allocated)),
    };
    if rows == 0 || columns == 0 || samples_per_pixel == 0 {
        return Err(RleDecodeError::InvalidLayout);
    }
    let samples_per_pixel =
        usize::try_from(samples_per_pixel).map_err(|_| RleDecodeError::LayoutSizeOverflow)?;
    let expected_segments = samples_per_pixel
        .checked_mul(bytes_per_sample)
        .ok_or(RleDecodeError::LayoutSizeOverflow)?;
    let pixel_count = usize::try_from(rows)
        .ok()
        .and_then(|rows| {
            usize::try_from(columns)
                .ok()
                .and_then(|columns| rows.checked_mul(columns))
        })
        .ok_or(RleDecodeError::LayoutSizeOverflow)?;
    let frame_len = pixel_count
        .checked_mul(expected_segments)
        .ok_or(RleDecodeError::LayoutSizeOverflow)?;

    let segment_count_u32 = read_u32_le(fragment, 0);
    if segment_count_u32 == 0 || segment_count_u32 as usize > RLE_MAX_SEGMENTS {
        return Err(RleDecodeError::InvalidSegmentCount(segment_count_u32));
    }
    let segment_count = segment_count_u32 as usize;
    if segment_count != expected_segments {
        return Err(RleDecodeError::SegmentCountMismatch {
            actual: segment_count,
            expected: expected_segments,
        });
    }

    let mut offsets = Vec::with_capacity(segment_count + 1);
    for segment in 0..segment_count {
        let offset = read_u32_le(fragment, 4 + segment * 4) as usize;
        if offset < RLE_HEADER_LEN || offset >= fragment.len() {
            return Err(RleDecodeError::InvalidSegmentOffset {
                segment,
                offset,
                length: fragment.len(),
            });
        }
        if segment == 0 && offset != RLE_HEADER_LEN {
            return Err(RleDecodeError::InvalidSegmentOffset {
                segment,
                offset,
                length: fragment.len(),
            });
        }
        if offsets.last().is_some_and(|previous| offset <= *previous) {
            return Err(RleDecodeError::NonIncreasingSegmentOffsets { segment });
        }
        offsets.push(offset);
    }
    for slot in segment_count..RLE_MAX_SEGMENTS {
        if read_u32_le(fragment, 4 + slot * 4) != 0 {
            return Err(RleDecodeError::NonZeroUnusedOffset { slot });
        }
    }
    offsets.push(fragment.len());

    let mut decoded_segments = Vec::with_capacity(segment_count);
    for segment in 0..segment_count {
        decoded_segments.push(decode_packbits_segment(
            &fragment[offsets[segment]..offsets[segment + 1]],
            pixel_count,
        )?);
    }

    let mut output = vec![0_u8; frame_len];
    for sample in 0..samples_per_pixel {
        for output_byte in 0..bytes_per_sample {
            // PS3.5 Annex G stores each sample's most-significant byte plane
            // first. Native raw frames use little-endian sample bytes, so the
            // segment order must be reversed within each sample.
            let segment_index = sample * bytes_per_sample + (bytes_per_sample - output_byte - 1);
            for (pixel, value) in decoded_segments[segment_index].iter().enumerate() {
                let output_index = pixel * samples_per_pixel * bytes_per_sample
                    + sample * bytes_per_sample
                    + output_byte;
                output[output_index] = *value;
            }
        }
    }
    Ok(output)
}

fn decode_packbits_segment(
    encoded: &[u8],
    expected_len: usize,
) -> std::result::Result<Vec<u8>, RleDecodeError> {
    let mut input = 0_usize;
    let mut output = Vec::with_capacity(expected_len);
    while output.len() < expected_len && input < encoded.len() {
        let control = encoded[input] as i8;
        input += 1;
        match control {
            0..=127 => {
                let count = control as usize + 1;
                if input + count > encoded.len() {
                    return Err(RleDecodeError::TruncatedLiteralRun);
                }
                if output.len() + count > expected_len {
                    return Err(RleDecodeError::SegmentOutputOverflow {
                        expected: expected_len,
                    });
                }
                output.extend_from_slice(&encoded[input..input + count]);
                input += count;
            }
            -127..=-1 => {
                if input >= encoded.len() {
                    return Err(RleDecodeError::TruncatedRepeatRun);
                }
                let count = 1_usize + usize::from(control.unsigned_abs());
                if output.len() + count > expected_len {
                    return Err(RleDecodeError::SegmentOutputOverflow {
                        expected: expected_len,
                    });
                }
                output.resize(output.len() + count, encoded[input]);
                input += 1;
            }
            -128 => {}
        }
    }
    if output.len() != expected_len {
        return Err(RleDecodeError::SegmentOutputLength {
            actual: output.len(),
            expected: expected_len,
        });
    }
    Ok(output)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn decode_monochrome_samples(
    decoded: &[u8],
    bits_allocated: u32,
    pixel_representation: u32,
) -> PixelResult<Vec<f64>> {
    match (bits_allocated, pixel_representation) {
        (8, 0) => Ok(decoded.iter().map(|value| f64::from(*value)).collect()),
        (8, 1) => Ok(decoded
            .iter()
            .map(|value| f64::from(*value as i8))
            .collect()),
        (16, 0) => Ok(decoded
            .chunks_exact(2)
            .map(|chunk| f64::from(u16::from_le_bytes([chunk[0], chunk[1]])))
            .collect()),
        (16, 1) => Ok(decoded
            .chunks_exact(2)
            .map(|chunk| f64::from(i16::from_le_bytes([chunk[0], chunk[1]])))
            .collect()),
        (_, representation) if representation > 1 => Err(PixelError::UnsupportedLayout(format!(
            "RLE PixelRepresentation {representation} is unsupported"
        ))),
        (bits, _) => Err(PixelError::UnsupportedLayout(format!(
            "RLE BitsAllocated {bits} is unsupported"
        ))),
    }
}

fn read_icc_profile(file: &FileEntry) -> PixelResult<Option<Vec<u8>>> {
    let object = open_file(&file.path)
        .with_context(|| format!("failed to open RLE DICOM: {}", file.path.display()))
        .map_err(PixelError::frame_decode)?;
    Ok(select_icc_profile(&object))
}

fn encode_rgb_png(
    file: &FileEntry,
    rgb: Vec<u8>,
    icc_profile: Option<Vec<u8>>,
) -> PixelResult<Bytes> {
    if file.bits_allocated != 8 {
        return Err(PixelError::UnsupportedLayout(format!(
            "RLE color display requires 8-bit samples, found {}",
            file.bits_allocated
        )));
    }
    encode_rgb8_png_with_icc(rgb, file.columns, file.rows, icc_profile)
        .context("RLE RGB PNG encoding failed")
        .map_err(PixelError::frame_decode)
}

fn normalize_color_for_display(
    decoded: &[u8],
    rows: u32,
    columns: u32,
    planar_configuration: u32,
    photometric: &str,
) -> PixelResult<Vec<u8>> {
    let pixel_count = usize::try_from(rows)
        .ok()
        .and_then(|rows| {
            usize::try_from(columns)
                .ok()
                .and_then(|columns| rows.checked_mul(columns))
        })
        .ok_or_else(|| {
            PixelError::UnsupportedLayout("RLE color geometry overflowed".to_string())
        })?;
    let normalized = match photometric {
        "RGB" => rgb8_interleaved(decoded, pixel_count, planar_configuration),
        "YBR_FULL" => ybr_full_to_rgb8(decoded, pixel_count, planar_configuration),
        _ => unreachable!("color normalization called for unsupported photometric interpretation"),
    };
    normalized
        .context("RLE color sample normalization failed")
        .map_err(PixelError::frame_decode)
}

fn encode_palette_png(file: &FileEntry, indices: &[u8]) -> PixelResult<Bytes> {
    if file.bits_allocated != 8 {
        return Err(PixelError::UnsupportedLayout(format!(
            "RLE palette display requires 8-bit indices, found {}",
            file.bits_allocated
        )));
    }
    let object = open_file(&file.path)
        .with_context(|| format!("failed to open RLE palette DICOM: {}", file.path.display()))
        .map_err(PixelError::frame_decode)?;
    let red = read_palette_channel(
        &object,
        "RedPaletteColorLookupTableDescriptor",
        "RedPaletteColorLookupTableData",
    )
    .map_err(PixelError::frame_decode)?;
    let green = read_palette_channel(
        &object,
        "GreenPaletteColorLookupTableDescriptor",
        "GreenPaletteColorLookupTableData",
    )
    .map_err(PixelError::frame_decode)?;
    let blue = read_palette_channel(
        &object,
        "BluePaletteColorLookupTableDescriptor",
        "BluePaletteColorLookupTableData",
    )
    .map_err(PixelError::frame_decode)?;
    if red.first_mapped != green.first_mapped
        || red.first_mapped != blue.first_mapped
        || red.values.len() != green.values.len()
        || red.values.len() != blue.values.len()
    {
        return Err(PixelError::UnsupportedLayout(
            "RLE palette channel descriptors do not match".to_string(),
        ));
    }

    let mut rgb = Vec::with_capacity(indices.len().saturating_mul(3));
    for index in indices {
        let mapped = i32::from(*index) - red.first_mapped;
        let lut_index = mapped.clamp(0, red.values.len().saturating_sub(1) as i32) as usize;
        rgb.extend_from_slice(&[
            red.values[lut_index],
            green.values[lut_index],
            blue.values[lut_index],
        ]);
    }
    let icc_profile = select_icc_profile(&object);
    encode_rgb_png(file, rgb, icc_profile)
}

struct PaletteChannel {
    first_mapped: i32,
    values: Vec<u8>,
}

fn read_palette_channel(
    object: &dicom_object::DefaultDicomObject,
    descriptor_name: &str,
    data_name: &str,
) -> Result<PaletteChannel> {
    let descriptor = object
        .element_by_name(descriptor_name)
        .with_context(|| format!("missing {descriptor_name}"))?
        .to_multi_int::<i32>()
        .with_context(|| format!("invalid {descriptor_name}"))?;
    if descriptor.len() != 3 {
        return Err(anyhow!("{descriptor_name} must contain three values"));
    }
    let entry_count = if descriptor[0] == 0 {
        65_536_usize
    } else {
        usize::try_from(descriptor[0]).context("negative palette entry count")?
    };
    let bits = descriptor[2];
    if bits != 8 && bits != 16 {
        return Err(anyhow!("unsupported palette bit depth {bits}"));
    }
    let words = object
        .element_by_name(data_name)
        .with_context(|| format!("missing {data_name}"))?
        .to_multi_int::<u16>()
        .with_context(|| format!("invalid {data_name}"))?;
    if words.len() < entry_count {
        return Err(anyhow!(
            "{data_name} contains {} entries, expected {entry_count}",
            words.len()
        ));
    }
    let values = words
        .into_iter()
        .take(entry_count)
        .map(|value| {
            if bits == 16 {
                (value >> 8) as u8
            } else {
                value as u8
            }
        })
        .collect();
    Ok(PaletteChannel {
        first_mapped: descriptor[1],
        values,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        decode_packbits_segment, decode_rle_frame, normalize_color_for_display, RleDecodeError,
        RLE_HEADER_LEN,
    };

    fn literal(values: &[u8]) -> Vec<u8> {
        assert!(!values.is_empty() && values.len() <= 128);
        let mut encoded = vec![(values.len() - 1) as u8];
        encoded.extend_from_slice(values);
        encoded
    }

    fn fragment(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut output = vec![0_u8; RLE_HEADER_LEN];
        output[0..4].copy_from_slice(&(segments.len() as u32).to_le_bytes());
        let mut offset = RLE_HEADER_LEN;
        for (index, segment) in segments.iter().enumerate() {
            output[4 + index * 4..8 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
            output.extend_from_slice(segment);
            offset += segment.len();
        }
        output
    }

    #[test]
    fn reconstructs_8_bit_grayscale_and_interleaved_rgb() {
        let mono = fragment(&[literal(&[0, 64, 128, 255])]);
        assert_eq!(
            decode_rle_frame(&mono, 2, 2, 1, 8).unwrap(),
            [0, 64, 128, 255]
        );

        let rgb = fragment(&[literal(&[255, 0]), literal(&[0, 255]), literal(&[10, 20])]);
        assert_eq!(
            decode_rle_frame(&rgb, 1, 2, 3, 8).unwrap(),
            [255, 0, 10, 0, 255, 20]
        );
    }

    #[test]
    fn normalizes_planar_rgb_and_ybr_for_display() {
        let planar_rgb = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        assert_eq!(
            normalize_color_for_display(&planar_rgb, 2, 2, 1, "RGB").unwrap(),
            [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]
        );

        let planar_ybr = [76, 150, 29, 255, 85, 44, 255, 128, 255, 21, 107, 128];
        assert_eq!(
            normalize_color_for_display(&planar_ybr, 2, 2, 1, "YBR_FULL").unwrap(),
            [254, 0, 0, 0, 255, 1, 0, 0, 254, 255, 255, 255]
        );
    }

    #[test]
    fn reconstructs_16_bit_byte_planes_in_little_endian_pixel_order() {
        let mono = fragment(&[literal(&[0x12, 0xab]), literal(&[0x34, 0xcd])]);
        assert_eq!(
            decode_rle_frame(&mono, 1, 2, 1, 16).unwrap(),
            [0x34, 0x12, 0xcd, 0xab]
        );

        let rgb = fragment(&[
            literal(&[0x11]),
            literal(&[0x12]),
            literal(&[0x21]),
            literal(&[0x22]),
            literal(&[0x31]),
            literal(&[0x32]),
        ]);
        assert_eq!(
            decode_rle_frame(&rgb, 1, 1, 3, 16).unwrap(),
            [0x12, 0x11, 0x22, 0x21, 0x32, 0x31]
        );
    }

    #[test]
    fn packbits_supports_literal_repeat_and_noop_runs() {
        let encoded = [0x80, 0x01, 1, 2, 0xfe, 9];
        assert_eq!(
            decode_packbits_segment(&encoded, 5).unwrap(),
            [1, 2, 9, 9, 9]
        );
    }

    #[test]
    fn rejects_invalid_headers_and_segment_tables() {
        assert_eq!(
            decode_rle_frame(&[0; 63], 1, 1, 1, 8),
            Err(RleDecodeError::HeaderTooShort)
        );

        let mut zero_segments = vec![0_u8; 65];
        zero_segments[64] = 0;
        assert_eq!(
            decode_rle_frame(&zero_segments, 1, 1, 1, 8),
            Err(RleDecodeError::InvalidSegmentCount(0))
        );
        assert_eq!(
            decode_rle_frame(&zero_segments, 0, 1, 1, 8),
            Err(RleDecodeError::InvalidLayout)
        );
        assert_eq!(
            decode_rle_frame(&zero_segments, 1, 1, 1, 32),
            Err(RleDecodeError::UnsupportedBitsAllocated(32))
        );

        let wrong_count = fragment(&[literal(&[1]), literal(&[2])]);
        assert_eq!(
            decode_rle_frame(&wrong_count, 1, 1, 1, 8),
            Err(RleDecodeError::SegmentCountMismatch {
                actual: 2,
                expected: 1
            })
        );

        let mut invalid_offset = fragment(&[literal(&[1])]);
        invalid_offset[4..8].copy_from_slice(&63_u32.to_le_bytes());
        assert!(matches!(
            decode_rle_frame(&invalid_offset, 1, 1, 1, 8),
            Err(RleDecodeError::InvalidSegmentOffset { .. })
        ));

        let mut unused_offset = fragment(&[literal(&[1])]);
        unused_offset[8..12].copy_from_slice(&64_u32.to_le_bytes());
        assert_eq!(
            decode_rle_frame(&unused_offset, 1, 1, 1, 8),
            Err(RleDecodeError::NonZeroUnusedOffset { slot: 1 })
        );

        let mut non_increasing = fragment(&[literal(&[1]), literal(&[2])]);
        non_increasing[8..12].copy_from_slice(&64_u32.to_le_bytes());
        assert_eq!(
            decode_rle_frame(&non_increasing, 1, 1, 1, 16),
            Err(RleDecodeError::NonIncreasingSegmentOffsets { segment: 1 })
        );
    }

    #[test]
    fn rejects_truncated_and_wrong_length_packbits_segments() {
        assert_eq!(
            decode_packbits_segment(&[2, 1, 2], 3),
            Err(RleDecodeError::TruncatedLiteralRun)
        );
        assert_eq!(
            decode_packbits_segment(&[0xff], 2),
            Err(RleDecodeError::TruncatedRepeatRun)
        );
        assert_eq!(
            decode_packbits_segment(&[1, 1, 2], 1),
            Err(RleDecodeError::SegmentOutputOverflow { expected: 1 })
        );
        assert_eq!(
            decode_packbits_segment(&[0, 1], 2),
            Err(RleDecodeError::SegmentOutputLength {
                actual: 1,
                expected: 2
            })
        );
    }
}
