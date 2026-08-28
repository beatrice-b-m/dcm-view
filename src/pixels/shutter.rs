use crate::types::RectangularDisplayShutter;

pub(crate) fn apply_rectangular_shutter(
    samples: &mut [u8],
    image_rows: u32,
    image_columns: u32,
    shutter: Option<RectangularDisplayShutter>,
) {
    let Some(shutter) = shutter else {
        return;
    };
    let presentation_value = p_value_to_u8(shutter.presentation_value);
    let columns = image_columns as usize;
    if columns == 0 {
        return;
    }

    for (index, sample) in samples.iter_mut().enumerate() {
        let row = index / columns;
        let column = index % columns;
        if row >= image_rows as usize {
            break;
        }
        let row = row as i64 + 1;
        let column = column as i64 + 1;
        let inside = column >= i64::from(shutter.left_vertical_edge)
            && column <= i64::from(shutter.right_vertical_edge)
            && row >= i64::from(shutter.upper_horizontal_edge)
            && row <= i64::from(shutter.lower_horizontal_edge);
        if !inside {
            *sample = presentation_value;
        }
    }
}

fn p_value_to_u8(value: u16) -> u8 {
    ((u32::from(value) * 255 + 32_767) / 65_535) as u8
}

#[cfg(test)]
mod tests {
    use super::{apply_rectangular_shutter, p_value_to_u8};
    use crate::types::RectangularDisplayShutter;

    #[test]
    fn prepared_full_frame_rectangle_preserves_display_pixels() {
        let mut samples = vec![0, 64, 128, 255];
        apply_rectangular_shutter(
            &mut samples,
            2,
            2,
            Some(RectangularDisplayShutter {
                left_vertical_edge: 1,
                right_vertical_edge: 2,
                upper_horizontal_edge: 1,
                lower_horizontal_edge: 2,
                presentation_value: 0,
            }),
        );
        assert_eq!(samples, [0, 64, 128, 255]);
    }

    #[test]
    fn non_degenerate_rectangle_replaces_every_pixel_outside_inclusive_edges() {
        let mut samples = (1_u8..=9).collect::<Vec<_>>();
        apply_rectangular_shutter(
            &mut samples,
            3,
            3,
            Some(RectangularDisplayShutter {
                left_vertical_edge: 2,
                right_vertical_edge: 2,
                upper_horizontal_edge: 2,
                lower_horizontal_edge: 2,
                presentation_value: 0,
            }),
        );
        assert_eq!(samples, [0, 0, 0, 0, 5, 0, 0, 0, 0]);
    }

    #[test]
    fn scales_unsigned_p_values_to_display_luminance() {
        assert_eq!(p_value_to_u8(0), 0);
        assert_eq!(p_value_to_u8(32_768), 128);
        assert_eq!(p_value_to_u8(65_535), 255);
    }
}
