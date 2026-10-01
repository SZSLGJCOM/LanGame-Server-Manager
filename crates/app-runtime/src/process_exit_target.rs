use std::io;

use app_core::ProcessIdentity;

use crate::{
    PROCESS_TERMINATE, RuntimeProcessError, TerminateProcess, WindowsProcessHandle,
    open_verified_windows_process,
};

/// An identity-verified process object retained for a bounded shutdown fallback.
/// It never reopens a PID, so an exited process cannot redirect termination to
/// an unrelated process that later receives the same PID.
pub struct ProcessExitTarget {
    process: WindowsProcessHandle,
}

// SAFETY: Windows permits concurrent waiting and termination on a process handle.
// This owner never replaces its handle, and Drop closes it only after all shared
// references end. No process memory or thread-affine resource is accessed.
unsafe impl Send for ProcessExitTarget {}
unsafe impl Sync for ProcessExitTarget {}

impl ProcessExitTarget {
    /// Capture this exact live process, or return `None` when it has exited.
    /// An identity mismatch is rejected before any termination request is possible.
    pub fn capture(pid: u32, expected_identity: &ProcessIdentity) -> io::Result<Option<Self>> {
        open_verified_windows_process(pid, expected_identity, PROCESS_TERMINATE)
            .map(|process| process.map(|process| Self { process }))
            .map_err(process_error)
    }

    pub fn is_running(&self) -> io::Result<bool> {
        self.process.is_running().map_err(process_error)
    }

    /// Request termination without waiting for cleanup or acquiring business locks.
    /// The caller owns the overall deadline and must observe completion separately.
    pub fn terminate(&self) -> io::Result<()> {
        if !self.is_running()? {
            return Ok(());
        }
        // SAFETY: Capture validated this owned handle and requested terminate
        // access. It stays open for the entire call, even if the process exits.
        if unsafe { TerminateProcess(self.process.raw, 1) } != 0 {
            return Ok(());
        }
        let source = io::Error::last_os_error();
        if self.is_running()? {
            Err(source)
        } else {
            // Concurrent normal exit is successful shutdown, not a kill failure.
            Ok(())
        }
    }
}

fn process_error(error: RuntimeProcessError) -> io::Error {
    match error {
        RuntimeProcessError::InspectProcess { source, .. }
        | RuntimeProcessError::WaitTrackedProcess { source, .. } => source,
        error @ RuntimeProcessError::ProcessIdentityMismatch { .. } => {
            io::Error::new(io::ErrorKind::PermissionDenied, error)
        }
        error => io::Error::other(error),
    }
}

#[cfg(test)]
#[path = "process_exit_target_tests.rs"]
pub(crate) mod tests;
