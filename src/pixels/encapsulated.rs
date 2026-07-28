use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use dicom_object::collector::DicomCollector;
use std::path::PathBuf;

pub(crate) fn read_encapsulated_fragment_blocking(path: &PathBuf, frame: u32) -> Result<Bytes> {
    let mut collector = DicomCollector::open_file(path).with_context(|| {
        format!(
            "failed to open DICOM for collector access: {}",
            path.display()
        )
    })?;

    let mut offset_table = Vec::<u32>::new();
    let _ = collector.read_basic_offset_table(&mut offset_table)?;
    if offset_table.iter().all(|offset| *offset == 0) {
        offset_table.clear();
    }

    let mut fragment = Vec::<u8>::new();
    for _ in 0..=frame {
        fragment.clear();
        collector
            .read_next_fragment(&mut fragment)?
            .ok_or_else(|| anyhow!("frame out of range"))?;
    }

    Ok(Bytes::from(fragment))
}
