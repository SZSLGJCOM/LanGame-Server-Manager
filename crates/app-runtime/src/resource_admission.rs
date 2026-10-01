use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use app_core::RuntimeResourceLimits;

const MIB: u64 = 1024 * 1024;

/// Owned by the runtime service, never a static. Weak entries do not keep a
/// stopped instance's Job or its memory reservation alive.
#[derive(Debug, Default)]
pub struct RuntimeResourceAdmission {
    groups: Mutex<BTreeMap<String, Weak<ResourceGroupInner>>>,
}

#[derive(Debug, Clone)]
pub struct RuntimeResourceGroup(Arc<ResourceGroupInner>);

#[derive(Debug)]
struct ResourceGroupInner {
    instance_id: String,
    limits: RuntimeResourceLimits,
    pending: AtomicBool,
    #[cfg(windows)]
    job: Option<crate::windows_process_job::OwnedProcessJob>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeResourceAdmissionSnapshot {
    pub active_groups: usize,
    pub reserved_memory_bytes: u64,
    pub pending_memory_bytes: u64,
    pub host_memory_reserve_bytes: u64,
}

impl RuntimeResourceAdmission {
    pub fn reserve(
        &self,
        instance_id: &str,
        limits: &RuntimeResourceLimits,
    ) -> io::Result<RuntimeResourceGroup> {
        self.reserve_with_memory(instance_id, limits, read_host_memory)
    }

    fn reserve_with_memory(
        &self,
        instance_id: &str,
        limits: &RuntimeResourceLimits,
        memory: impl FnOnce() -> io::Result<(u64, u64)>,
    ) -> io::Result<RuntimeResourceGroup> {
        limits.validate().map_err(invalid)?;
        let mut groups = self
            .groups
            .lock()
            .map_err(|_| io::Error::other("resource admission lock poisoned"))?;
        groups.retain(|_, group| group.strong_count() > 0);
        if groups.get(instance_id).and_then(Weak::upgrade).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("instance `{instance_id}` still owns a resource reservation"),
            ));
        }
        let existing = snapshot_groups(&groups);
        if let Some(memory_mib) = limits.memory_limit_mib {
            // Observe memory while holding admission: concurrent launches cannot
            // spend the same available memory before their processes exist.
            let (total, available) = memory()?;
            check_memory_admission(
                existing,
                memory_mib * MIB,
                limits.host_memory_reserve_mib * MIB,
                total,
                available,
            )?;
        }
        let group = RuntimeResourceGroup::new(instance_id, limits.clone())?;
        groups.insert(instance_id.to_owned(), Arc::downgrade(&group.0));
        Ok(group)
    }

    pub fn snapshot(&self) -> io::Result<RuntimeResourceAdmissionSnapshot> {
        let groups = self
            .groups
            .lock()
            .map_err(|_| io::Error::other("resource admission lock poisoned"))?;
        Ok(snapshot_groups(&groups))
    }

    /// The applied startup limits, distinct from settings edited for a future
    /// launch. None means this service owns no resource group for the instance.
    pub fn limits_for_instance(
        &self,
        instance_id: &str,
    ) -> io::Result<Option<RuntimeResourceLimits>> {
        let groups = self
            .groups
            .lock()
            .map_err(|_| io::Error::other("resource admission lock poisoned"))?;
        Ok(groups
            .get(instance_id)
            .and_then(Weak::upgrade)
            .map(|group| group.limits.clone()))
    }
}

fn snapshot_groups(
    groups: &BTreeMap<String, Weak<ResourceGroupInner>>,
) -> RuntimeResourceAdmissionSnapshot {
    let mut snapshot = RuntimeResourceAdmissionSnapshot::default();
    for group in groups.values().filter_map(Weak::upgrade) {
        snapshot.active_groups += 1;
        if let Some(memory_mib) = group.limits.memory_limit_mib {
            let bytes = memory_mib * MIB;
            snapshot.reserved_memory_bytes = snapshot.reserved_memory_bytes.saturating_add(bytes);
            if group.pending.load(Ordering::Acquire) {
                snapshot.pending_memory_bytes = snapshot.pending_memory_bytes.saturating_add(bytes);
            }
            snapshot.host_memory_reserve_bytes = snapshot
                .host_memory_reserve_bytes
                .max(group.limits.host_memory_reserve_mib * MIB);
        }
    }
    snapshot
}

fn check_memory_admission(
    existing: RuntimeResourceAdmissionSnapshot,
    requested: u64,
    reserve: u64,
    total: u64,
    available: u64,
) -> io::Result<()> {
    if total == 0 || available > total {
        return Err(io::Error::other(
            "host physical memory could not be measured reliably",
        ));
    }
    let reserve = reserve.max(existing.host_memory_reserve_bytes);
    let committed = existing
        .reserved_memory_bytes
        .checked_add(requested)
        .ok_or_else(|| invalid("memory budget overflow"))?;
    if committed > total.saturating_sub(reserve) {
        return Err(io::Error::other(format!(
            "Configured instance memory budgets would total {} MiB, exceeding {} MiB physical memory after reserving {} MiB for the host",
            committed / MIB,
            total.saturating_sub(reserve) / MIB,
            reserve / MIB
        )));
    }
    let pending = existing
        .pending_memory_bytes
        .checked_add(requested)
        .ok_or_else(|| invalid("pending memory budget overflow"))?;
    if pending > available.saturating_sub(reserve) {
        return Err(io::Error::other(format!(
            "Startup needs {} MiB for pending instance budgets, but only {} MiB is available after reserving {} MiB for the host",
            pending / MIB,
            available.saturating_sub(reserve) / MIB,
            reserve / MIB
        )));
    }
    Ok(())
}

impl RuntimeResourceGroup {
    fn new(instance_id: &str, limits: RuntimeResourceLimits) -> io::Result<Self> {
        #[cfg(not(windows))]
        if limits.enabled() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Instance CPU and memory limits require Windows Job Objects",
            ));
        }
        #[cfg(windows)]
        let job = if limits.enabled() {
            let job = crate::windows_process_job::OwnedProcessJob::new()?;
            job.apply_resource_limits(&limits)?;
            Some(job)
        } else {
            None
        };
        Ok(Self(Arc::new(ResourceGroupInner {
            instance_id: instance_id.to_owned(),
            limits,
            pending: AtomicBool::new(true),
            #[cfg(windows)]
            job,
        })))
    }

    /// Call only after every initial world/process has been spawned. The full
    /// logical budget stays reserved until the last process owner is released.
    pub fn mark_started(&self) {
        self.0.pending.store(false, Ordering::Release);
    }
    pub fn limits(&self) -> &RuntimeResourceLimits {
        &self.0.limits
    }
    pub fn instance_id(&self) -> &str {
        &self.0.instance_id
    }

    #[cfg(windows)]
    pub(super) fn job_handle(&self) -> Option<*mut std::ffi::c_void> {
        self.0
            .job
            .as_ref()
            .map(crate::windows_process_job::OwnedProcessJob::as_raw)
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(windows)]
fn read_host_memory() -> io::Result<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((status.ullTotalPhys, status.ullAvailPhys))
}

#[cfg(not(windows))]
fn read_host_memory() -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows host memory information is required",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_and_physical_availability_are_distinct_checks() {
        let running = RuntimeResourceAdmissionSnapshot {
            reserved_memory_bytes: 8192 * MIB,
            ..Default::default()
        };
        assert!(
            check_memory_admission(running, 4096 * MIB, 2048 * MIB, 16384 * MIB, 8192 * MIB)
                .is_ok()
        );
        assert!(
            check_memory_admission(running, 8192 * MIB, 2048 * MIB, 16384 * MIB, 16384 * MIB)
                .is_err()
        );
        assert!(
            check_memory_admission(running, 4096 * MIB, 2048 * MIB, 16384 * MIB, 4096 * MIB)
                .is_err()
        );
    }

    #[test]
    fn pending_starts_cannot_reuse_the_same_free_memory() {
        let pending = RuntimeResourceAdmissionSnapshot {
            reserved_memory_bytes: 4096 * MIB,
            pending_memory_bytes: 4096 * MIB,
            ..Default::default()
        };
        assert!(
            check_memory_admission(pending, 4096 * MIB, 2048 * MIB, 32768 * MIB, 8192 * MIB)
                .is_err()
        );
    }

    #[test]
    fn reservations_are_not_retained_by_the_service_registry() {
        let admission = RuntimeResourceAdmission::default();
        let group = admission
            .reserve_with_memory("a", &RuntimeResourceLimits::default(), || {
                panic!("no budget must not inspect memory")
            })
            .unwrap();
        let process_owner = group.clone();
        drop(group);
        assert_eq!(admission.snapshot().unwrap().active_groups, 1);
        assert!(
            admission
                .reserve("a", &RuntimeResourceLimits::default())
                .is_err()
        );
        drop(process_owner);
        assert_eq!(
            admission.snapshot().unwrap(),
            RuntimeResourceAdmissionSnapshot::default()
        );
        assert!(
            admission
                .reserve("a", &RuntimeResourceLimits::default())
                .is_ok()
        );
    }
}
