//! Stable, SOP-Class-based identity for DICOM object families.
//!
//! This module deliberately classifies only SOP Classes that the viewer knows
//! by identity. Classification does not imply pixel or semantic support; it is
//! an observability primitive for describing what kind of object was opened.

use std::fmt;

/// A coarse DICOM object family derived from an exact SOP Class UID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    ClassicImage,
    EnhancedImage,
    WholeSlideMicroscopy,
    Segmentation,
    ParametricMap,
    RealWorldValueMapping,
    PresentationState,
    Registration,
    StructuredReport,
    KeyObjectSelection,
    Waveform,
    RadiationTherapy,
    EncapsulatedPdf,
    Unknown,
}

impl ObjectKind {
    /// Return the stable snake_case identifier used for evidence and APIs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClassicImage => "classic_image",
            Self::EnhancedImage => "enhanced_image",
            Self::WholeSlideMicroscopy => "whole_slide_microscopy",
            Self::Segmentation => "segmentation",
            Self::ParametricMap => "parametric_map",
            Self::RealWorldValueMapping => "real_world_value_mapping",
            Self::PresentationState => "presentation_state",
            Self::Registration => "registration",
            Self::StructuredReport => "structured_report",
            Self::KeyObjectSelection => "key_object_selection",
            Self::Waveform => "waveform",
            Self::RadiationTherapy => "radiation_therapy",
            Self::EncapsulatedPdf => "encapsulated_pdf",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Classify a DICOM SOP Class UID without inferring support or renderability.
///
/// Exact matching is intentional. An unrecognized UID, including another UID
/// beneath a familiar numeric prefix, remains [`ObjectKind::Unknown`].
pub fn classify_sop_class(sop_class_uid: &str) -> ObjectKind {
    match sop_class_uid {
        // Prepared classic and visible-light image families.
        "1.2.840.10008.5.1.4.1.1.1"
        | "1.2.840.10008.5.1.4.1.1.1.1"
        | "1.2.840.10008.5.1.4.1.1.1.2"
        | "1.2.840.10008.5.1.4.1.1.1.2.1"
        | "1.2.840.10008.5.1.4.1.1.2"
        | "1.2.840.10008.5.1.4.1.1.3.1"
        | "1.2.840.10008.5.1.4.1.1.4"
        | "1.2.840.10008.5.1.4.1.1.6.1"
        | "1.2.840.10008.5.1.4.1.1.7"
        | "1.2.840.10008.5.1.4.1.1.7.1"
        | "1.2.840.10008.5.1.4.1.1.12.1"
        | "1.2.840.10008.5.1.4.1.1.12.2"
        | "1.2.840.10008.5.1.4.1.1.20"
        | "1.2.840.10008.5.1.4.1.1.77.1.1"
        | "1.2.840.10008.5.1.4.1.1.77.1.2"
        | "1.2.840.10008.5.1.4.1.1.77.1.4"
        | "1.2.840.10008.5.1.4.1.1.128" => ObjectKind::ClassicImage,

        // Prepared enhanced image families. Concatenation membership is an
        // instance-level identity layered on top of this object kind.
        "1.2.840.10008.5.1.4.1.1.2.1"
        | "1.2.840.10008.5.1.4.1.1.4.1"
        | "1.2.840.10008.5.1.4.1.1.130" => ObjectKind::EnhancedImage,

        "1.2.840.10008.5.1.4.1.1.77.1.6" => ObjectKind::WholeSlideMicroscopy,

        "1.2.840.10008.5.1.4.1.1.66.4" | "1.2.840.10008.5.1.4.1.1.66.7" => ObjectKind::Segmentation,
        "1.2.840.10008.5.1.4.1.1.30" => ObjectKind::ParametricMap,
        "1.2.840.10008.5.1.4.1.1.67" => ObjectKind::RealWorldValueMapping,

        "1.2.840.10008.5.1.4.1.1.11.1"
        | "1.2.840.10008.5.1.4.1.1.11.2"
        | "1.2.840.10008.5.1.4.1.1.11.4"
        | "1.2.840.10008.5.1.4.1.1.11.8" => ObjectKind::PresentationState,

        "1.2.840.10008.5.1.4.1.1.66.1" | "1.2.840.10008.5.1.4.1.1.66.3" => ObjectKind::Registration,

        "1.2.840.10008.5.1.4.1.1.88.11"
        | "1.2.840.10008.5.1.4.1.1.88.33"
        | "1.2.840.10008.5.1.4.1.1.88.34" => ObjectKind::StructuredReport,
        "1.2.840.10008.5.1.4.1.1.88.59" => ObjectKind::KeyObjectSelection,

        "1.2.840.10008.5.1.4.1.1.9.1.1" | "1.2.840.10008.5.1.4.1.1.9.1.2" => ObjectKind::Waveform,

        "1.2.840.10008.5.1.4.1.1.481.1"
        | "1.2.840.10008.5.1.4.1.1.481.2"
        | "1.2.840.10008.5.1.4.1.1.481.3"
        | "1.2.840.10008.5.1.4.1.1.481.5"
        | "1.2.840.10008.5.1.4.1.1.481.12"
        | "1.2.840.10008.5.1.4.1.1.481.13" => ObjectKind::RadiationTherapy,

        "1.2.840.10008.5.1.4.1.1.104.1" => ObjectKind::EncapsulatedPdf,
        _ => ObjectKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_sop_class, ObjectKind};

    const PREPARED_SOP_CLASSES: &[(&str, ObjectKind)] = &[
        ("1.2.840.10008.5.1.4.1.1.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.1.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.1.2", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.1.2.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.2", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.2.1", ObjectKind::EnhancedImage),
        ("1.2.840.10008.5.1.4.1.1.3.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.4", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.4.1", ObjectKind::EnhancedImage),
        ("1.2.840.10008.5.1.4.1.1.6.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.7", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.7.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.9.1.1", ObjectKind::Waveform),
        ("1.2.840.10008.5.1.4.1.1.9.1.2", ObjectKind::Waveform),
        (
            "1.2.840.10008.5.1.4.1.1.11.1",
            ObjectKind::PresentationState,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.11.2",
            ObjectKind::PresentationState,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.11.4",
            ObjectKind::PresentationState,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.11.8",
            ObjectKind::PresentationState,
        ),
        ("1.2.840.10008.5.1.4.1.1.12.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.12.2", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.20", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.30", ObjectKind::ParametricMap),
        ("1.2.840.10008.5.1.4.1.1.66.1", ObjectKind::Registration),
        ("1.2.840.10008.5.1.4.1.1.66.3", ObjectKind::Registration),
        ("1.2.840.10008.5.1.4.1.1.66.4", ObjectKind::Segmentation),
        ("1.2.840.10008.5.1.4.1.1.66.7", ObjectKind::Segmentation),
        (
            "1.2.840.10008.5.1.4.1.1.67",
            ObjectKind::RealWorldValueMapping,
        ),
        ("1.2.840.10008.5.1.4.1.1.77.1.1", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.77.1.2", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.77.1.4", ObjectKind::ClassicImage),
        (
            "1.2.840.10008.5.1.4.1.1.77.1.6",
            ObjectKind::WholeSlideMicroscopy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.88.11",
            ObjectKind::StructuredReport,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.88.33",
            ObjectKind::StructuredReport,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.88.34",
            ObjectKind::StructuredReport,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.88.59",
            ObjectKind::KeyObjectSelection,
        ),
        ("1.2.840.10008.5.1.4.1.1.104.1", ObjectKind::EncapsulatedPdf),
        ("1.2.840.10008.5.1.4.1.1.128", ObjectKind::ClassicImage),
        ("1.2.840.10008.5.1.4.1.1.130", ObjectKind::EnhancedImage),
        (
            "1.2.840.10008.5.1.4.1.1.481.1",
            ObjectKind::RadiationTherapy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.481.2",
            ObjectKind::RadiationTherapy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.481.3",
            ObjectKind::RadiationTherapy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.481.5",
            ObjectKind::RadiationTherapy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.481.12",
            ObjectKind::RadiationTherapy,
        ),
        (
            "1.2.840.10008.5.1.4.1.1.481.13",
            ObjectKind::RadiationTherapy,
        ),
    ];

    #[test]
    fn classifies_every_prepared_sop_class() {
        assert_eq!(PREPARED_SOP_CLASSES.len(), 44);
        for &(uid, expected) in PREPARED_SOP_CLASSES {
            assert_eq!(classify_sop_class(uid), expected, "SOP Class UID {uid}");
        }
    }

    #[test]
    fn exposes_stable_snake_case_names() {
        let kinds = [
            (ObjectKind::ClassicImage, "classic_image"),
            (ObjectKind::EnhancedImage, "enhanced_image"),
            (ObjectKind::WholeSlideMicroscopy, "whole_slide_microscopy"),
            (ObjectKind::Segmentation, "segmentation"),
            (ObjectKind::ParametricMap, "parametric_map"),
            (
                ObjectKind::RealWorldValueMapping,
                "real_world_value_mapping",
            ),
            (ObjectKind::PresentationState, "presentation_state"),
            (ObjectKind::Registration, "registration"),
            (ObjectKind::StructuredReport, "structured_report"),
            (ObjectKind::KeyObjectSelection, "key_object_selection"),
            (ObjectKind::Waveform, "waveform"),
            (ObjectKind::RadiationTherapy, "radiation_therapy"),
            (ObjectKind::EncapsulatedPdf, "encapsulated_pdf"),
            (ObjectKind::Unknown, "unknown"),
        ];

        for (kind, expected) in kinds {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn falls_back_to_unknown_without_prefix_inference() {
        for uid in [
            "",
            "1.2.840.10008.5.1.4.1.1",
            "1.2.840.10008.5.1.4.1.1.66.99",
            "1.2.840.10008.5.1.4.1.1.481.999",
            "1.2.840.10008.5.1.4.1.1.2 ",
            "9.9.9.9",
        ] {
            assert_eq!(classify_sop_class(uid), ObjectKind::Unknown, "UID {uid:?}");
        }
    }
}
