use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::ManagedProcess;
use crate::{RuntimeProcessError, RuntimeStdin, RuntimeStdinCancellation};

#[derive(Debug, Clone)]
pub struct RuntimeCommandDispatchTarget {
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeCommandSubmissionTracker {
    submitted: Arc<AtomicBool>,
}

impl RuntimeCommandSubmissionTracker {
    pub fn mark_submitted(&self) {
        self.submitted.store(true, Ordering::Release);
    }

    pub fn is_submitted(&self) -> bool {
        self.submitted.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub struct RuntimeCommandDispatchLease {
    instance_id: String,
    target: RuntimeCommandDispatchTarget,
    cancellation: RuntimeStdinCancellation,
    stdin: Option<RuntimeStdin>,
    returned: SyncSender<RuntimeStdin>,
}

impl RuntimeCommandDispatchLease {
    pub fn target(&self) -> &RuntimeCommandDispatchTarget {
        &self.target
    }

    pub fn write_stdin_line(&mut self, command: &str) -> Result<(), RuntimeProcessError> {
        self.stdin
            .as_mut()
            .ok_or(RuntimeProcessError::MissingTrackedProcessStdin {
                pid: self.target.pid,
            })?
            .write_stdin_line_with_cancellation(command, &self.cancellation)
            .map_err(|source| RuntimeProcessError::WriteTrackedProcessStdin {
                pid: self.target.pid,
                source,
            })
    }

    pub(super) fn return_stdin(mut self) -> String {
        if let Some(stdin) = self.stdin.take() {
            // The capacity-one return slot belongs to the exact instance owner.
            // A disconnected receiver means that owner was dropped; the rejected
            // value closes here, without extending the process or instance lifetime.
            let _ = self.returned.try_send(stdin);
        }
        self.instance_id
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct DispatchKey {
    run_id: i64,
    process_key: String,
    root_pid: u32,
}

impl DispatchKey {
    fn matches(&self, process: &ManagedProcess) -> bool {
        self.run_id == process.run_id
            && self.process_key.eq_ignore_ascii_case(&process.process_key)
            && process
                .child
                .as_ref()
                .is_some_and(|child| child.id() == self.root_pid)
    }
}

#[derive(Debug)]
struct PendingDispatch {
    cancellation: RuntimeStdinCancellation,
    returned: Receiver<RuntimeStdin>,
}

impl Drop for PendingDispatch {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

/// Lives with ManagedInstance even while a stop worker temporarily owns it.
/// One channel per dispatch supplies an ownership generation independent of PID reuse.
#[derive(Debug, Default)]
pub(super) struct StdinDispatchRegistry {
    pending: HashMap<DispatchKey, PendingDispatch>,
}

impl StdinDispatchRegistry {
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn begin(
        &mut self,
        instance_id: &str,
        process: &mut ManagedProcess,
    ) -> Result<RuntimeCommandDispatchLease, RuntimeProcessError> {
        let child = process
            .child
            .as_mut()
            .ok_or(RuntimeProcessError::MissingTrackedProcessStdin { pid: process.pid })?;
        let key = DispatchKey {
            run_id: process.run_id,
            process_key: process.process_key.to_ascii_lowercase(),
            root_pid: child.id(),
        };
        let stdin = child
            .take_stdin()
            .ok_or(RuntimeProcessError::MissingTrackedProcessStdin { pid: process.pid })?;
        let cancellation = RuntimeStdinCancellation::default();
        let (sender, returned) = sync_channel(1);
        self.pending.insert(
            key,
            PendingDispatch {
                cancellation: cancellation.clone(),
                returned,
            },
        );
        Ok(RuntimeCommandDispatchLease {
            instance_id: instance_id.to_owned(),
            target: RuntimeCommandDispatchTarget {
                process_key: process.process_key.clone(),
                display_name: process.display_name.clone(),
                pid: process.pid,
            },
            cancellation,
            stdin: Some(stdin),
            returned: sender,
        })
    }

    pub(super) fn cancel_all(&self) {
        for dispatch in self.pending.values() {
            dispatch.cancellation.cancel();
        }
    }

    pub(super) fn remove_process(&mut self, process: &ManagedProcess) {
        self.pending.retain(|key, _| !key.matches(process));
    }

    pub(super) fn restore_returned(&mut self, processes: &mut [ManagedProcess]) {
        self.pending.retain(|key, dispatch| {
            match dispatch.returned.try_recv() {
                Ok(stdin) => {
                    if stdin_is_healthy(&stdin)
                        && let Some(child) = processes
                            .iter_mut()
                            .find(|process| key.matches(process))
                            .and_then(|process| process.child.as_mut())
                    {
                        child.restore_stdin(stdin);
                    }
                    false
                }
                Err(TryRecvError::Empty) => true,
                // A dropped lease closes its stdin; no tombstone survives it.
                Err(TryRecvError::Disconnected) => false,
            }
        });
    }
}

fn stdin_is_healthy(stdin: &RuntimeStdin) -> bool {
    #[cfg(windows)]
    match stdin {
        RuntimeStdin::Standard(pipe) => pipe.is_healthy(),
        RuntimeStdin::Windows(pipe) => pipe.is_healthy(),
        // The terminal has its own shared failure state and rejects reuse itself.
        RuntimeStdin::PseudoConsole(_) => true,
    }
    #[cfg(not(windows))]
    {
        let _ = stdin;
        true
    }
}
