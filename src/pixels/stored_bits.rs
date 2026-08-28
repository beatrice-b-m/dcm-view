use anyhow::{bail, Result};

/// Canonicalize integer pixel cells to little-endian, allocated-width values.
///
/// DICOM does not define the contents of bits outside the stored sample field.
/// Mask unsigned samples and sign-extend signed samples so display and raw
/// consumers never interpret those padding bits as pixel data.
pub(crate) fn canonicalize_integer_samples(
    bytes: &mut [u8],
    bits_allocated: u32,
    bits_stored: Option<u32>,
    high_bit: Option<u32>,
    signed: bool,
) -> Result<()> {
    let bits_stored = bits_stored.unwrap_or(bits_allocated);
    let high_bit = high_bit.unwrap_or_else(|| bits_stored.saturating_sub(1));
    if bits_allocated == 0
        || !matches!(bits_allocated, 8 | 16 | 32)
        || bits_stored == 0
        || bits_stored > bits_allocated
        || high_bit >= bits_allocated
        || high_bit + 1 < bits_stored
    {
        bail!(
            "invalid integer pixel bit field: BitsAllocated {bits_allocated}, BitsStored {bits_stored}, HighBit {high_bit}"
        );
    }

    let bytes_per_sample = (bits_allocated / 8) as usize;
    if !bytes.len().is_multiple_of(bytes_per_sample) {
        bail!(
            "integer pixel buffer length {} is not divisible by {bytes_per_sample}",
            bytes.len()
        );
    }
    if bits_stored == bits_allocated && high_bit + 1 == bits_allocated {
        return Ok(());
    }

    let low_bit = high_bit + 1 - bits_stored;
    let stored_mask = (1_u64 << bits_stored) - 1;
    let allocated_mask = (1_u64 << bits_allocated) - 1;
    let sign_bit = 1_u64 << (bits_stored - 1);

    for sample in bytes.chunks_exact_mut(bytes_per_sample) {
        let mut value = sample
            .iter()
            .enumerate()
            .fold(0_u64, |value, (shift, byte)| {
                value | (u64::from(*byte) << (shift * 8))
            });
        value = (value >> low_bit) & stored_mask;
        if signed && value & sign_bit != 0 {
            value |= allocated_mask ^ stored_mask;
        }
        for (shift, byte) in sample.iter_mut().enumerate() {
            *byte = (value >> (shift * 8)) as u8;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::canonicalize_integer_samples;

    #[test]
    fn masks_unsigned_and_sign_extends_signed_samples() {
        let mut unsigned = [0xBC, 0xFA];
        canonicalize_integer_samples(&mut unsigned, 16, Some(12), Some(11), false).unwrap();
        assert_eq!(unsigned, [0xBC, 0x0A]);

        let mut signed = [0xFF, 0xAF];
        canonicalize_integer_samples(&mut signed, 16, Some(12), Some(11), true).unwrap();
        assert_eq!(signed, [0xFF, 0xFF]);
    }

    #[test]
    fn shifts_legacy_high_bit_placements_before_extension() {
        let mut signed = [0xF0, 0xFF];
        canonicalize_integer_samples(&mut signed, 16, Some(12), Some(15), true).unwrap();
        assert_eq!(signed, [0xFF, 0xFF]);

        let mut unsigned32 = [0xF0, 0xDE, 0xBC, 0xFA];
        canonicalize_integer_samples(&mut unsigned32, 32, Some(28), Some(31), false).unwrap();
        assert_eq!(unsigned32, [0xEF, 0xCD, 0xAB, 0x0F]);
    }

    #[test]
    fn rejects_inconsistent_bit_fields_and_buffer_lengths() {
        assert!(canonicalize_integer_samples(&mut [0; 2], 16, Some(17), Some(16), false).is_err());
        assert!(canonicalize_integer_samples(&mut [0; 1], 16, Some(12), Some(11), false).is_err());
    }
}
