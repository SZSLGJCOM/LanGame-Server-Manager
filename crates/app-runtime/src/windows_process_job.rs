use std::collections::BTreeMap;
use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_MORE_DATA};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};

use crate::{OwnedWindowsHandle, RuntimeChild};

const MAX_WAITED_PROCESSES: usize = 4096;

#[repr(C)]
struct ProcessIdList {
    assigned: u32,
    count: u32,
    ids: [usize; MAX_WAITED_PROCESSES],
}

/// An anonymous launch container. Its handle is never inherited by workloads,
/// so loss of the manager's final owner also terminates remaining descendants.
#[derive(Debug)]
pub(super) struct OwnedProcessJob(
    OwnedWindowsHandle,
    Mutex<BTreeMap<u32, OwnedWindowsHandle>>,
    Option<crate::RuntimeResourceGroup>,
);

// Job handles support cross-thread queries and termination. Configuration is
// complete before publication; only the unique owner closes the handle.
unsafe impl Send for OwnedProcessJob {}
unsafe impl Sync for OwnedProcessJob {}

impl OwnedProcessJob {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: Null attributes request a non-inheritable handle; the anonymous
        // object cannot collide with another instance or the outer build Job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let owner = Self(OwnedWindowsHandle::new(handle), Mutex::default(), None);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: The owned handle and typed buffer remain valid for this call.
        if unsafe {
            SetInformationJobObject(
                owner.as_raw(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(owner)
    }

    pub(super) fn new_in_resource_group(
        group: Option<&crate::RuntimeResourceGroup>,
    ) -> io::Result<Self> {
        let mut job = Self::new()?;
        job.2 = group.cloned();
        Ok(job)
    }

    /// Parent first: every world shares one budget, but keeps its own child Job
    /// so cleaning up one failed shard never terminates surviving siblings.
    pub(super) fn launch_job_handles(&self) -> Box<[*mut std::ffi::c_void]> {
        self.2
            .as_ref()
            .and_then(crate::RuntimeResourceGroup::job_handle)
            .into_iter()
            .chain(std::iter::once(self.as_raw()))
            .collect()
    }

    pub(super) fn apply_resource_limits(
        &self,
        policy: &app_core::RuntimeResourceLimits,
    ) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::{
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            JOB_OBJECT_LIMIT_JOB_MEMORY, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
            JobObjectCpuRateControlInformation,
        };
        policy
            .validate()
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(mib) = policy.memory_limit_mib {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = usize::try_from(mib * 1024 * 1024).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Memory limit does not fit the host address size",
                )
            })?;
        }
        if unsafe {
            SetInformationJobObject(
                self.as_raw(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::other(format!(
                "Cannot apply instance memory limit: {}",
                io::Error::last_os_error()
            )));
        }
        if let Some(percent) = policy.cpu_percent {
            let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                ..Default::default()
            };
            cpu.Anonymous.CpuRate = u32::from(percent) * 100;
            if unsafe {
                SetInformationJobObject(
                    self.as_raw(),
                    JobObjectCpuRateControlInformation,
                    (&cpu as *const JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                    std::mem::size_of_val(&cpu) as u32,
                )
            } == 0
            {
                return Err(io::Error::other(format!(
                    "Cannot apply instance CPU hard cap: {}",
                    io::Error::last_os_error()
                )));
            }
        }
        Ok(())
    }

    pub(super) fn as_raw(&self) -> *mut std::ffi::c_void {
        self.0.as_raw()
    }

    pub(super) fn active_process_count(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: This exact accounting class writes the supplied typed buffer.
        if unsafe {
            QueryInformationJobObject(
                self.as_raw(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&accounting) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(accounting.ActiveProcesses)
    }

    fn is_running(&self) -> io::Result<bool> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut processes = self.1.try_lock().map_err(|error| {
            io::Error::other(format!(
                "managed process tree inspection ownership is unavailable: {error}"
            ))
        })?;
        // Keep verified handles between polls: accounting can reach zero before
        // those process objects are signaled, including an exited launcher's children.
        self.capture_processes(&mut processes, deadline)?;
        let mut running = self.active_process_count()? != 0;
        for process in processes.values() {
            ensure_before_deadline(deadline)?;
            match unsafe { crate::WaitForSingleObject(process.as_raw(), 0) } {
                crate::WAIT_OBJECT_0 => {}
                crate::WAIT_TIMEOUT => running = true,
                _ => return Err(io::Error::last_os_error()),
            }
        }
        Ok(running)
    }

    pub(super) fn terminate_and_wait(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut processes = self.1.try_lock().map_err(|error| {
            io::Error::other(format!(
                "managed process tree cleanup ownership is unavailable: {error}"
            ))
        })?;
        // Accounting can reach zero before process handles become signaled.
        // Retain verified handles across failure/retry, even after they disappear
        // from the Job's active PID list. Never wait on a reopened PID alone.
        self.capture_processes(&mut processes, deadline)?;
        if self.active_process_count()? != 0 {
            self.terminate()?;
        }
        loop {
            ensure_before_deadline(deadline)?;
            // A member may have spawned between the initial capture and
            // termination. Re-query and terminate newly observed members too.
            let added = self.capture_processes(&mut processes, deadline)?;
            let active = self.active_process_count()?;
            if added && active != 0 {
                self.terminate()?;
            }
            let mut all_exited = true;
            for process in processes.values() {
                ensure_before_deadline(deadline)?;
                match unsafe { crate::WaitForSingleObject(process.as_raw(), 0) } {
                    crate::WAIT_OBJECT_0 => {}
                    crate::WAIT_TIMEOUT => all_exited = false,
                    _ => return Err(io::Error::last_os_error()),
                }
            }
            if active == 0 && all_exited {
                processes.clear();
                return Ok(());
            }
            std::thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn terminate(&self) -> io::Result<()> {
        // SAFETY: This is the private launch Job, never the manager/parent Job.
        if unsafe { TerminateJobObject(self.as_raw(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn capture_processes(
        &self,
        processes: &mut BTreeMap<u32, OwnedWindowsHandle>,
        deadline: Instant,
    ) -> io::Result<bool> {
        ensure_before_deadline(deadline)?;
        let mut list = ProcessIdList {
            assigned: 0,
            count: 0,
            ids: [0; MAX_WAITED_PROCESSES],
        };
        // SAFETY: The repr(C) buffer follows JOBOBJECT_BASIC_PROCESS_ID_LIST's
        // two DWORD header fields and aligned variable ULONG_PTR array.
        if unsafe {
            QueryInformationJobObject(
                self.as_raw(),
                JobObjectBasicProcessIdList,
                (&mut list as *mut ProcessIdList).cast(),
                std::mem::size_of_val(&list) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                io::Error::other("managed process tree exceeds the 4096-process cleanup bound")
            } else {
                error
            });
        }
        if list.assigned > list.count || list.count as usize > MAX_WAITED_PROCESSES {
            return Err(io::Error::other(
                "managed process tree PID list is incomplete",
            ));
        }
        let mut added = false;
        for pid in &list.ids[..list.count as usize] {
            ensure_before_deadline(deadline)?;
            let pid = u32::try_from(*pid)
                .map_err(|_| io::Error::other("managed process tree returned an invalid PID"))?;
            if processes.contains_key(&pid) {
                continue;
            }
            if processes.len() == MAX_WAITED_PROCESSES {
                return Err(io::Error::other(
                    "managed process tree exceeds the 4096-process cleanup bound",
                ));
            }
            let raw = unsafe {
                crate::OpenProcess(
                    crate::SYNCHRONIZE | crate::PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if raw.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    continue; // This PID exited between the Job query and open.
                }
                return Err(error);
            }
            let process = OwnedWindowsHandle::new(raw);
            let mut member = 0;
            // SAFETY: Validate membership on the opened process object, so PID
            // reuse cannot turn an unrelated process into an owned wait target.
            if unsafe { IsProcessInJob(process.as_raw(), self.as_raw(), &mut member) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if member != 0 {
                processes.insert(pid, process);
                added = true;
            }
        }
        Ok(added)
    }
}

fn ensure_before_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "managed process tree operation did not complete within two seconds",
        ))
    } else {
        Ok(())
    }
}

impl RuntimeChild {
    pub(crate) fn owned_process_tree_is_running(&mut self) -> io::Result<Option<bool>> {
        let Self::Windows(child) = self else {
            return Ok(None);
        };
        if let Some(elevated) = &mut child.elevated {
            return elevated.is_running(child.process_handle).map(Some);
        }
        let Some(job) = &child.job else {
            return Ok(None);
        };
        let tree_running = job.is_running()?;
        let root_running = child.try_wait()?.is_none();
        Ok(Some(tree_running || root_running))
    }

    pub(crate) fn has_owned_process_tree(&self) -> bool {
        matches!(self, Self::Windows(child) if child.job.is_some() || child.elevated.is_some())
    }

    /// Reap remaining descendants before reporting a run stopped. Failure keeps
    /// the launch owner alive so the caller can restore ownership and retry.
    pub fn finish_process_tree(&mut self) -> io::Result<()> {
        if let Self::Windows(child) = self
            && let Some(elevated) = &mut child.elevated
        {
            return elevated.finish(child.process_handle);
        }
        if let Self::Windows(child) = self
            && let Some(job) = &child.job
        {
            job.terminate_and_wait()?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn restrict_job_to_query_for_test(
    job: &mut OwnedProcessJob,
) -> io::Result<OwnedProcessJob> {
    const JOB_OBJECT_QUERY_ACCESS: u32 = 0x0004;
    restrict_job_access_for_test(job, JOB_OBJECT_QUERY_ACCESS)
}

#[cfg(test)]
pub(super) fn restrict_job_to_terminate_for_test(
    job: &mut OwnedProcessJob,
) -> io::Result<OwnedProcessJob> {
    const JOB_OBJECT_TERMINATE_ACCESS: u32 = 0x0008;
    restrict_job_access_for_test(job, JOB_OBJECT_TERMINATE_ACCESS)
}

#[cfg(test)]
fn restrict_job_access_for_test(
    job: &mut OwnedProcessJob,
    desired_access: u32,
) -> io::Result<OwnedProcessJob> {
    // Duplicate the same kernel object with restricted permissions, preserving
    // the full-access owner for recovery after exercising inspection or cleanup.
    let current_process = unsafe { crate::GetCurrentProcess() };
    let mut raw = std::ptr::null_mut();
    if unsafe {
        crate::DuplicateHandle(
            current_process,
            job.as_raw(),
            current_process,
            &mut raw,
            desired_access,
            0,
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let resource_group = job.2.clone();
    Ok(std::mem::replace(
        job,
        OwnedProcessJob(
            OwnedWindowsHandle::new(raw),
            Mutex::default(),
            resource_group,
        ),
    ))
}
