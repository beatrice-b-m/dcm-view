use crate::api::contracts::{WindowMode, WindowPreset};
use crate::types::ResolvedWindow;

pub fn resolve_window(
    requested_wc: Option<f64>,
    requested_ww: Option<f64>,
    default_window: Option<WindowPreset>,
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
    default_window: Option<WindowPreset>,
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
