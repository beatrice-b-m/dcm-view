use crate::api::contracts::FileSummary;
use crate::loader::{DiscoveryDisposition, DiscoveryRecord};
use crate::types::FileEntry;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::{futures::Notified, Notify};

#[derive(Clone)]
pub struct FileRegistry {
    inner: Arc<RwLock<FileRegistryInner>>,
    scanned: Arc<AtomicUsize>,
    skipped: Arc<AtomicUsize>,
    filtered: Arc<AtomicUsize>,
    scan_complete: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[derive(Default)]
struct FileRegistryInner {
    files: Vec<FileEntry>,
    summaries: Vec<FileSummary>,
    discovery_ledger: Vec<DiscoveryRecord>,
}

#[derive(Debug, Clone, Copy)]
pub struct RegistryStatus {
    pub file_count: usize,
    pub scanned: usize,
    pub skipped: usize,
    pub filtered: usize,
    pub scan_complete: bool,
}

impl FileRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FileRegistryInner::default())),
            scanned: Arc::new(AtomicUsize::new(0)),
            skipped: Arc::new(AtomicUsize::new(0)),
            filtered: Arc::new(AtomicUsize::new(0)),
            scan_complete: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn from_files(files: Vec<FileEntry>) -> Self {
        let registry = Self::new();
        for file in files {
            registry.insert(file);
            registry.record_scanned();
        }
        registry.mark_scan_complete();
        registry
    }

    pub fn insert(&self, mut file: FileEntry) -> usize {
        let mut inner = self.inner.write().expect("file registry lock poisoned");
        let index = inner.files.len();
        file.index = index;
        let summary = FileSummary::from(&file);
        inner.files.push(file);
        inner.summaries.push(summary);
        drop(inner);
        self.notify.notify_waiters();
        index
    }

    pub fn record_scanned(&self) {
        self.scanned.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_filtered(&self) {
        self.filtered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_discovery(&self, record: DiscoveryRecord) {
        match record.disposition {
            DiscoveryDisposition::Selected => self.record_scanned(),
            DiscoveryDisposition::Skipped => self.record_skipped(),
            DiscoveryDisposition::Filtered => self.record_filtered(),
        }
        self.inner
            .write()
            .expect("file registry lock poisoned")
            .discovery_ledger
            .push(record);
        self.notify.notify_waiters();
    }

    pub fn mark_scan_complete(&self) {
        self.scan_complete.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    pub fn changed(&self) -> Notified<'_> {
        self.notify.notified()
    }

    pub fn get(&self, index: usize) -> Option<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .get(index)
            .cloned()
    }

    pub fn files_snapshot(&self) -> Vec<FileEntry> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .clone()
    }

    pub fn summaries_snapshot(&self) -> Vec<FileSummary> {
        self.inner
            .read()
            .expect("file registry lock poisoned")
            .summaries
            .clone()
    }

    pub fn discovery_ledger_snapshot(&self) -> Vec<DiscoveryRecord> {
        let mut records = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .discovery_ledger
            .clone();
        records.sort();
        records
    }

    pub fn status(&self) -> RegistryStatus {
        let file_count = self
            .inner
            .read()
            .expect("file registry lock poisoned")
            .files
            .len();
        RegistryStatus {
            file_count,
            scanned: self.scanned.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            filtered: self.filtered.load(Ordering::Relaxed),
            scan_complete: self.scan_complete.load(Ordering::Relaxed),
        }
    }
}

impl Default for FileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::FileRegistry;
    use crate::loader::{DiscoveryDisposition, DiscoveryReason, DiscoveryRecord};
    use std::path::PathBuf;

    #[test]
    fn discovery_ledger_is_memory_only_sorted_and_updates_counts() {
        let registry = FileRegistry::new();
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/z-invalid.bin"),
            disposition: DiscoveryDisposition::Skipped,
            reason: DiscoveryReason::MissingPart10Preamble,
        });
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/a-selected.dcm"),
            disposition: DiscoveryDisposition::Selected,
            reason: DiscoveryReason::ValidDicom,
        });
        registry.record_discovery(DiscoveryRecord {
            path: PathBuf::from("/scan/m-filtered.dcm"),
            disposition: DiscoveryDisposition::Filtered,
            reason: DiscoveryReason::FilterMismatch,
        });

        let records = registry.discovery_ledger_snapshot();
        assert_eq!(
            records
                .iter()
                .map(|record| record.path.as_path())
                .collect::<Vec<_>>(),
            vec![
                std::path::Path::new("/scan/a-selected.dcm"),
                std::path::Path::new("/scan/m-filtered.dcm"),
                std::path::Path::new("/scan/z-invalid.bin"),
            ]
        );
        assert_eq!(records[0].reason.code(), "valid_dicom");
        assert_eq!(records[1].reason.code(), "filter_mismatch");
        assert_eq!(records[2].reason.code(), "missing_part10_preamble");

        let status = registry.status();
        assert_eq!(status.scanned, 1);
        assert_eq!(status.skipped, 1);
        assert_eq!(status.filtered, 1);

        let clone = registry.clone();
        assert_eq!(clone.discovery_ledger_snapshot(), records);
        assert!(FileRegistry::new().discovery_ledger_snapshot().is_empty());
    }
}
