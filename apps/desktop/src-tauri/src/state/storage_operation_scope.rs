use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{DesktopState, StorageContextOperationGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StorageOperationScope {
    MigratablePaths,
    FixedApplicationData,
}

#[derive(Debug)]
pub(super) struct ActiveStorageOperation {
    pub name: String,
    pub scope: StorageOperationScope,
    pub cancellation: Arc<AtomicBool>,
}

impl DesktopState {
    /// Only for work confined to a captured app-data subdirectory and read-only
    /// bundled modules. It must not bootstrap storage or use any configurable
    /// root. Migration protects those fixed locations; shutdown drains both scopes.
    pub(crate) fn begin_fixed_application_data_operation(
        &self,
        operation: &str,
    ) -> Result<StorageContextOperationGuard, String> {
        self.begin_scoped_storage_operation(operation, StorageOperationScope::FixedApplicationData)
    }
}

pub(super) fn path_operation_blockers(
    active: &HashMap<u64, ActiveStorageOperation>,
) -> Option<String> {
    const MAX_NAMES: usize = 8;
    const MAX_NAME_CHARS: usize = 96;
    let mut count = 0;
    let mut names = BTreeSet::new();
    for operation in active.values() {
        if operation.scope != StorageOperationScope::MigratablePaths {
            continue;
        }
        count += 1;
        let mut chars = operation.name.chars().filter(|value| !value.is_control());
        let mut name = chars.by_ref().take(MAX_NAME_CHARS).collect::<String>();
        if chars.next().is_some() {
            name.push('…');
        }
        names.insert(name);
        if names.len() > MAX_NAMES {
            names.pop_last();
        }
    }
    (count > 0).then(|| {
        format!(
            "暂时无法更改存储位置，仍有 {count} 项操作正在使用这些目录（{}）。请等待操作完成后重试。",
            names.into_iter().collect::<Vec<_>>().join("、")
        )
    })
}

#[cfg(test)]
#[path = "storage_operation_scope_tests.rs"]
mod tests;
