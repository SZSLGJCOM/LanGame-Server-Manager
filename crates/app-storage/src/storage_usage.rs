use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{StorageError, StoragePaths};

#[path = "storage_usage_native.rs"]
mod native;
#[path = "storage_usage_plan.rs"]
mod plan;

const MAX_ENTRIES: u64 = 500_000;
const MAX_DEPTH: usize = 64;
const MAX_SCAN_DURATION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct StorageUsageEntry {
    pub id: String,
    pub category: String,
    pub label: String,
    pub path: String,
    pub instance_id: Option<String>,
    pub module_id: Option<String>,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub file_count: u64,
    pub status: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StorageUsageReport {
    pub scan_id: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub status: String,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub file_count: u64,
    pub skipped_links: u64,
    pub entries: Vec<StorageUsageEntry>,
    pub issues: Vec<String>,
}

pub(crate) struct ScanPlan {
    entries: Vec<StorageUsageEntry>,
    roots: Vec<PathBuf>,
    assignments: HashMap<String, usize>,
    issues: Vec<String>,
    cancellation: Arc<AtomicBool>,
    deadline: Instant,
}

/// Read-only, bounded metadata inspection. The caller retains its storage lease
/// until this future settles, including the blocking filesystem worker.
pub async fn scan_storage_usage(
    paths: &StoragePaths,
    scan_id: String,
    cancellation: Arc<AtomicBool>,
) -> Result<StorageUsageReport, StorageError> {
    let started_at = timestamp();
    let plan = plan::build(
        paths,
        cancellation.clone(),
        Instant::now() + MAX_SCAN_DURATION,
    )
    .await?;
    tokio::task::spawn_blocking(move || scan(plan, scan_id, started_at, cancellation))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "measuring managed storage",
            message: error.to_string(),
        })
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        text.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

struct Scanner {
    plan: ScanPlan,
    cancellation: Arc<AtomicBool>,
    deadline: Instant,
    examined: u64,
    skipped_links: u64,
    stopped: bool,
    identities: HashSet<[u64; 3]>,
}

fn scan(
    plan: ScanPlan,
    scan_id: String,
    started_at_unix_ms: u64,
    cancellation: Arc<AtomicBool>,
) -> StorageUsageReport {
    let deadline = plan.deadline;
    let mut scanner = Scanner {
        plan,
        cancellation,
        deadline,
        examined: 0,
        skipped_links: 0,
        stopped: false,
        identities: HashSet::new(),
    };
    scanner.should_stop();
    let roots = scanner.plan.roots.clone();
    for root in roots {
        let owner = scanner.plan.assignments[&path_key(&root)];
        if scanner.should_stop() {
            break;
        }
        let _ancestors = match native::pin_ancestors(&root) {
            Ok(pins) => pins,
            Err(error) => {
                scanner.issue(owner, &root, error);
                continue;
            }
        };
        scanner.walk(&root, owner, 0);
        if scanner.stopped {
            break;
        }
    }
    let cancelled = scanner.cancellation.load(Ordering::Acquire);
    if scanner.stopped {
        for entry in &mut scanner.plan.entries {
            if entry.status == "complete" {
                entry.status = "partial".into();
            }
        }
    }
    let partial = scanner.stopped
        || !scanner.plan.issues.is_empty()
        || scanner
            .plan
            .entries
            .iter()
            .any(|entry| entry.status == "partial");
    let entries = scanner.plan.entries;
    StorageUsageReport {
        scan_id,
        started_at_unix_ms,
        finished_at_unix_ms: timestamp(),
        status: if cancelled {
            "cancelled"
        } else if partial {
            "partial"
        } else {
            "complete"
        }
        .into(),
        logical_bytes: entries.iter().fold(0u64, |total, entry| {
            total.saturating_add(entry.logical_bytes)
        }),
        allocated_bytes: entries.iter().try_fold(0u64, |total, entry| {
            entry
                .allocated_bytes
                .map(|bytes| total.saturating_add(bytes))
        }),
        file_count: entries.iter().map(|entry| entry.file_count).sum(),
        skipped_links: scanner.skipped_links,
        entries,
        issues: scanner.plan.issues,
    }
}

impl Scanner {
    fn should_stop(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        let reason = if self.cancellation.load(Ordering::Acquire) {
            Some("Storage scan cancelled; the displayed values are partial.")
        } else if self.examined >= MAX_ENTRIES {
            Some("Storage scan reached its 500,000-entry limit; the displayed values are partial.")
        } else if Instant::now() >= self.deadline {
            Some("Storage scan reached its 60-second limit; the displayed values are partial.")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.plan.issues.push(reason.into());
            self.stopped = true;
        }
        self.stopped
    }

    fn issue(&mut self, owner: usize, path: &Path, error: impl std::fmt::Display) {
        let entry = &mut self.plan.entries[owner];
        entry.status = "partial".into();
        if entry.issues.len() < 8 {
            entry.issues.push(format!("{}: {error}", path.display()));
        }
    }

    fn walk(&mut self, path: &Path, inherited_owner: usize, depth: usize) {
        if self.should_stop() {
            return;
        }
        let owner = self
            .plan
            .assignments
            .get(&path_key(path))
            .copied()
            .unwrap_or(inherited_owner);
        self.examined += 1;
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.issue(owner, path, error);
                return;
            }
        };
        if native::is_link(&metadata) {
            self.skipped_links += 1;
            self.issue(
                owner,
                path,
                "symbolic links and reparse points are excluded",
            );
            return;
        }
        if metadata.is_file() {
            match native::file_usage(path, &metadata) {
                Ok(usage) => {
                    let entry = &mut self.plan.entries[owner];
                    entry.logical_bytes = entry.logical_bytes.saturating_add(usage.logical);
                    entry.file_count += 1;
                    // One allocated file is counted once even when several managed
                    // directories contain hard links. This is not reclaimable space.
                    if usage
                        .identity
                        .is_none_or(|identity| self.identities.insert(identity))
                    {
                        entry.allocated_bytes = entry
                            .allocated_bytes
                            .zip(usage.allocated)
                            .map(|(total, bytes)| total.saturating_add(bytes));
                    }
                }
                Err(error) => self.issue(owner, path, error),
            }
            return;
        }
        if !metadata.is_dir() {
            self.issue(owner, path, "unsupported filesystem entry");
            return;
        }
        if depth >= MAX_DEPTH {
            self.issue(owner, path, "directory depth exceeds the scan limit");
            return;
        }
        let _pin = match native::pin_directory(path) {
            Ok(pin) => pin,
            Err(error) => {
                self.issue(owner, path, error);
                return;
            }
        };
        let children = match fs::read_dir(path) {
            Ok(children) => children,
            Err(error) => {
                self.issue(owner, path, error);
                return;
            }
        };
        for child in children {
            if self.should_stop() {
                return;
            }
            match child {
                Ok(child) => self.walk(&child.path(), owner, depth + 1),
                Err(error) => self.issue(owner, path, error),
            }
        }
    }
}

#[cfg(test)]
#[path = "storage_usage_tests.rs"]
mod tests;
