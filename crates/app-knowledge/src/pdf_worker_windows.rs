use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, TryLockError, mpsc};
use std::time::{Duration, Instant};

use super::{MAGIC, MAX_INPUT, read_worker_output};
use crate::{KnowledgeError, Result, check_cancel};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};

const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(10);
static PARSER_SLOT: Mutex<()> = Mutex::new(());

struct Job(OwnedHandle);
impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: Null attributes create an anonymous, non-inheritable Job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW transferred one valid owned handle.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_JOB_MEMORY;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = MEMORY_LIMIT;
        limits.JobMemoryLimit = MEMORY_LIMIT;
        // SAFETY: The handle and typed limit structure remain alive for the call.
        if unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: Both handles are owned live objects. No input has been sent.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn terminate(&self) {
        // SAFETY: The Job handle is valid and owned by this parser invocation.
        unsafe {
            TerminateJobObject(self.0.as_raw_handle(), 1);
        }
    }
}

struct Worker {
    child: Child,
    job: Job,
    reaped: bool,
}
impl Worker {
    fn stop_and_wait(&mut self) -> io::Result<()> {
        if !self.reaped {
            self.job.terminate();
            // Assignment may have failed; kill the known process in that case.
            let _ = self.child.kill();
            self.child.wait()?;
            self.reaped = true;
        }
        Ok(())
    }
    fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop_and_wait();
    }
}

pub(super) fn verify_current_job() -> io::Result<()> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: A null handle queries the current process's associated Job; the
    // correctly sized output structure is valid for the duration of this call.
    if unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let required = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_JOB_MEMORY
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    if limits.BasicLimitInformation.LimitFlags & required != required
        || limits.BasicLimitInformation.ActiveProcessLimit != 1
        || limits.ProcessMemoryLimit == 0
        || limits.ProcessMemoryLimit > MEMORY_LIMIT
        || limits.JobMemoryLimit == 0
        || limits.JobMemoryLimit > MEMORY_LIMIT
    {
        return Err(io::Error::other(
            "PDF worker lacks required process isolation",
        ));
    }
    Ok(())
}

pub(super) fn run(
    mut command: Command,
    bytes: &[u8],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<String> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return Err(KnowledgeError::Invalid(
            "PDF input is empty or exceeds 8 MiB".into(),
        ));
    }
    let _slot = loop {
        check_cancel(cancel)?;
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        match PARSER_SLOT.try_lock() {
            Ok(slot) => break slot,
            Err(TryLockError::WouldBlock) => std::thread::sleep(POLL),
            Err(TryLockError::Poisoned(_)) => {
                return Err(KnowledgeError::Unavailable(
                    "PDF parser admission lock poisoned".into(),
                ));
            }
        }
    };
    // The Worker is constructed inside the scope: even an unwind drops it and
    // closes the child before scoped pipe threads are joined, avoiding deadlock.
    std::thread::scope(|scope| {
        let job = Job::new()?;
        let child = command.spawn()?;
        let mut worker = Worker {
            child,
            job,
            reaped: false,
        };
        worker.job.assign(&worker.child)?;
        let mut input = worker
            .child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Missing PDF worker input pipe"))?;
        let output = worker
            .child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Missing PDF worker output pipe"))?;
        let writer = scope.spawn(move || -> io::Result<()> {
            input.write_all(MAGIC)?;
            input.write_all(&(bytes.len() as u64).to_le_bytes())?;
            input.write_all(bytes)?;
            input.flush()
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = scope.spawn(move || {
            let _ = sender.send(read_worker_output(output));
        });
        let mut response = None;
        let result = loop {
            if let Err(error) = check_cancel(cancel) {
                break Err(error);
            }
            if Instant::now() >= deadline {
                break Err(timed_out());
            }
            if response.is_none() {
                match receiver.try_recv() {
                    Ok(Err(error)) => break Err(error),
                    Ok(Ok(parsed)) => response = Some(parsed),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err(KnowledgeError::Unavailable(
                            "PDF worker response thread failed".into(),
                        ));
                    }
                }
            }
            match worker.status() {
                Ok(Some(status)) => {
                    // An exited worker cannot retain the pipe, and its Job
                    // forbids descendants. Drain the bounded frame through EOF
                    // before interpreting a nonzero exit as a process failure.
                    let completed = match response {
                        Some(parsed) => Ok(parsed),
                        None => receiver.recv().map_err(|_| {
                            KnowledgeError::Unavailable("PDF worker response is missing".into())
                        })?,
                    };
                    break if status.success() {
                        completed.and_then(std::convert::identity)
                    } else {
                        match completed {
                            Ok(Err(error)) => Err(error),
                            _ => Err(KnowledgeError::Unavailable(format!(
                                "PDF worker exited unsuccessfully ({status}); parsing or its memory limit failed"
                            ))),
                        }
                    };
                }
                Ok(None) => {}
                Err(error) => break Err(error.into()),
            }
            std::thread::sleep(POLL);
        };
        // Always settle the OS process before joining I/O and releasing the slot.
        let cleanup = worker.stop_and_wait();
        let written = writer
            .join()
            .map_err(|_| KnowledgeError::Unavailable("PDF input thread failed".into()))?;
        reader
            .join()
            .map_err(|_| KnowledgeError::Unavailable("PDF response thread failed".into()))?;
        cleanup?;
        if result.is_ok() {
            written?;
        }
        result
    })
}

fn timed_out() -> KnowledgeError {
    KnowledgeError::Unavailable(
        "PDF parsing timed out; the worker was terminated and reaped".into(),
    )
}
