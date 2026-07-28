use crate::api::contracts::{TagNode, TagValue};
use anyhow::{Context, Result};
use dicom_core::dictionary::{DataDictionary, DataDictionaryEntry};
use dicom_core::header::HasLength;
use dicom_dictionary_std::StandardDataDictionary;
use dicom_object::{open_file, InMemDicomObject};
use std::path::Path;

const TAG_TEXT_PREVIEW_LIMIT: usize = 256;
const TAG_NUMERIC_VALUE_LIMIT: usize = 128;
const TAG_SEQUENCE_ITEM_LIMIT: usize = 64;
const TAG_SEQUENCE_DEPTH_LIMIT: usize = 4;

pub(crate) fn build_tag_tree(path: &Path) -> Result<Vec<TagNode>> {
    let object = open_file(path)
        .with_context(|| format!("failed to open DICOM for tags: {}", path.display()))?
        .into_inner();
    Ok(serialize_object_tags(&object, 0))
}

fn serialize_object_tags(
    object: &InMemDicomObject<StandardDataDictionary>,
    depth: usize,
) -> Vec<TagNode> {
    object
        .iter()
        .map(|element| serialize_element(element, depth))
        .collect()
}

fn serialize_element(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
    depth: usize,
) -> TagNode {
    let tag = element.header().tag;
    let tag_repr = format!("({:04X},{:04X})", tag.0, tag.1);
    let vr_repr = format!("{}", element.header().vr());
    let keyword = StandardDataDictionary
        .by_tag(tag)
        .map(|entry| entry.alias().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let value = serialize_tag_value(element, tag_repr.as_str(), &vr_repr, depth);

    TagNode {
        tag: tag_repr,
        vr: vr_repr,
        keyword,
        value,
    }
}

fn serialize_tag_value(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
    tag_repr: &str,
    vr_repr: &str,
    depth: usize,
) -> TagValue {
    if tag_repr == "(7FE0,0010)" {
        return binary_value_from_element(element);
    }

    if vr_repr == "SQ" {
        return match element.items() {
            Some(items) => serialize_sequence_items(items, depth),
            None => TagValue::Error {
                message: "sequence item decoding failed".to_string(),
            },
        };
    }

    if matches!(vr_repr, "OB" | "OW" | "OD" | "OF" | "UN" | "OL") {
        return binary_value_from_element(element);
    }

    let string_value = match element.to_str() {
        Ok(value) => value.to_string(),
        Err(error) => {
            return TagValue::Error {
                message: format!("value serialization failed: {error}"),
            };
        }
    };

    if is_numeric_vr(vr_repr) {
        let mut numbers = string_value
            .split('\\')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect::<Vec<_>>();
        if numbers.is_empty() {
            TagValue::Error {
                message: "numeric conversion failed".to_string(),
            }
        } else if numbers.len() == 1 {
            TagValue::Number { value: numbers[0] }
        } else {
            let total = numbers.len();
            let truncated = total > TAG_NUMERIC_VALUE_LIMIT;
            numbers.truncate(TAG_NUMERIC_VALUE_LIMIT);
            TagValue::Numbers {
                value: numbers,
                truncated,
                total: truncated.then_some(total),
            }
        }
    } else {
        TagValue::String {
            value: format_tag_text_preview(&string_value),
        }
    }
}

fn serialize_sequence_items(
    items: &[InMemDicomObject<StandardDataDictionary>],
    depth: usize,
) -> TagValue {
    let total = items.len();
    let depth_limited = depth >= TAG_SEQUENCE_DEPTH_LIMIT;
    let item_limited = total > TAG_SEQUENCE_ITEM_LIMIT;

    if depth_limited {
        return TagValue::Sequence {
            items: Vec::new(),
            truncated: true,
            total: Some(total),
        };
    }

    let serialized_items = items
        .iter()
        .take(TAG_SEQUENCE_ITEM_LIMIT)
        .map(|item| serialize_object_tags(item, depth + 1))
        .collect();

    TagValue::Sequence {
        items: serialized_items,
        truncated: item_limited,
        total: item_limited.then_some(total),
    }
}

fn format_tag_text_preview(raw: &str) -> String {
    let normalized = raw.replace('\\', "; ");
    let mut chars = normalized.chars();
    let preview: String = chars.by_ref().take(TAG_TEXT_PREVIEW_LIMIT).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn binary_value_from_element(
    element: &dicom_object::mem::InMemElement<StandardDataDictionary>,
) -> TagValue {
    if let Some(length) = element.header().length().get() {
        return TagValue::Binary {
            length: length as usize,
        };
    }

    if let Some(fragments) = element.fragments() {
        let length = fragments.iter().map(|fragment| fragment.len()).sum();
        return TagValue::Binary { length };
    }

    match element.to_bytes() {
        Ok(bytes) => TagValue::Binary {
            length: bytes.len(),
        },
        Err(error) => TagValue::Error {
            message: format!("binary serialization failed: {error}"),
        },
    }
}

fn is_numeric_vr(vr_repr: &str) -> bool {
    matches!(
        vr_repr,
        "US" | "SS" | "UL" | "SL" | "FL" | "FD" | "DS" | "IS"
    )
}
