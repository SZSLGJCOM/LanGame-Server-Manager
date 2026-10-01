use std::collections::VecDeque;
use std::path::Path;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::{JoinError, JoinHandle};

use super::{
    CHILD_REAP_TIMEOUT, InstallDeadline, InstallDeadlineElapsed, SteamCmdError, apply_no_window,
    operation_timeout,
};

#[cfg(windows)]
use std::collections::BTreeMap;
#[cfg(windows)]
use std::mem::{size_of, zeroed};
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::ptr::{null, null_mut};
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA, PROCESS_TERMINATE, ResumeThread, THREAD_SUSPEND_RESUME, WaitForSingleObject,
};

const COMMAND_OUTPUT_CAPTURE_LIMIT_BYTES: usize = 256 * 1024;
const COMMAND_OUTPUT_TRUNCATED_PREFIX: &[u8] =
    b"[earlier command output truncated; retained tail follows]\n";
const COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES: usize =
    COMMAND_OUTPUT_CAPTURE_LIMIT_BYTES - COMMAND_OUTPUT_TRUNCATED_PREFIX.len();
const OUTPUT_READ_CHUNK_BYTES: usize = 8 * 1024;

pub(super) struct AbortOnDropTask<T> {
    handle: Option<JoinHandle<T>>,
}

impl<T> AbortOnDropTask<T> {
    pub(super) fn new(handle: JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    pub(super) fn abort(&self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.abort();
        }
    }

    pub(super) async fn join(&mut self) -> Result<T, JoinError> {
        let result = self
            .handle
            .as_mut()
            .expect("owned task can only be joined once")
            .await;
        self.handle = None;
        result
    }

    pub(super) async fn abort_and_join(&mut self) {
        let Some(handle) = self.handle.as_mut() else {
            return;
        };
        handle.abort();
        let _ = handle.await;
        self.handle = None;
    }

    #[cfg(test)]
    pub(super) fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl<T> Drop for AbortOnDropTask<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.as_ref() {
            handle.abort();
        }
    }
}

struct BoundedOutputTail {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl BoundedOutputTail {
    fn new() -> Self {
        Self {
            bytes: VecDeque::new(),
            truncated: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if chunk.len() >= COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES {
            self.truncated |=
                !self.bytes.is_empty() || chunk.len() > COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES;
            self.bytes.clear();
            self.bytes.extend(
                chunk[chunk.len() - COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES..]
                    .iter()
                    .copied(),
            );
            return;
        }

        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES);
        if overflow > 0 {
            self.bytes.drain(..overflow).for_each(drop);
            self.truncated = true;
        }
        self.bytes.extend(chunk.iter().copied());
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut output = Vec::with_capacity(
            self.bytes.len()
                + if self.truncated {
                    COMMAND_OUTPUT_TRUNCATED_PREFIX.len()
                } else {
                    0
                },
        );
        if self.truncated {
            output.extend_from_slice(COMMAND_OUTPUT_TRUNCATED_PREFIX);
        }
        output.extend(self.bytes);
        output
    }
}

pub(super) struct ChildProcessGuard {
    #[cfg(windows)]
    job: Option<WindowsJob>,
}

impl ChildProcessGuard {
    async fn wait_for_empty(&self, deadline: tokio::time::Instant) -> std::io::Result<()> {
        #[cfg(windows)]
        if let Some(job) = self.job.as_ref() {
            loop {
                job.observe_processes(deadline)?;
                let empty = job.is_empty()?;
                let exited = job.observed_processes_exited(deadline)?;
                if empty && exited {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        #[cfg(not(windows))]
        let _ = deadline;
        Ok(())
    }

    pub(super) async fn wait_until_empty(
        &self,
        deadline: InstallDeadline,
    ) -> Result<(), SteamCmdError> {
        deadline
            .run(self.wait_for_empty(deadline.expires_at()))
            .await
            .map_err(|InstallDeadlineElapsed| operation_timeout(deadline))?
            .map_err(|source| SteamCmdError::SpawnCommand { source })
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: OwnedHandle,
    observed: Mutex<BTreeMap<u32, OwnedHandle>>,
}

#[cfg(windows)]
const MAX_CLEANUP_PROCESSES: usize = 4096;

#[cfg(windows)]
#[repr(C)]
struct JobProcessIds {
    assigned: u32,
    count: u32,
    ids: [usize; MAX_CLEANUP_PROCESSES],
}

#[cfg(windows)]
impl WindowsJob {
    fn new() -> std::io::Result<Self> {
        let raw = unsafe {
            // SAFETY: null attributes and name create one private Job Object.
            CreateJobObjectW(null(), null())
        };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let handle = unsafe {
            // SAFETY: CreateJobObjectW returned one owned Job Object handle.
            OwnedHandle::from_raw_handle(raw)
        };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe {
            // SAFETY: this Windows policy structure is valid when zero-initialized.
            zeroed()
        };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            // SAFETY: handle and limits remain valid for this synchronous call.
            SetInformationJobObject(
                handle.as_raw_handle().cast(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            observed: Mutex::default(),
        })
    }

    fn assign_and_resume(&self, pid: u32) -> std::io::Result<()> {
        let process_raw = unsafe {
            // SAFETY: pid came from the suspended child just created below.
            OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid)
        };
        if process_raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let process = unsafe {
            // SAFETY: OpenProcess returned one owned process handle.
            OwnedHandle::from_raw_handle(process_raw)
        };
        let assigned = unsafe {
            // SAFETY: both handles are valid and the child is still suspended.
            AssignProcessToJobObject(
                self.handle.as_raw_handle().cast(),
                process.as_raw_handle().cast(),
            )
        };
        if assigned == 0 {
            return Err(std::io::Error::last_os_error());
        }

        resume_suspended_process_threads(pid)
    }

    fn is_empty(&self) -> std::io::Result<bool> {
        let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe {
            // SAFETY: the accounting structure is valid when zero-initialized.
            zeroed()
        };
        let queried = unsafe {
            // SAFETY: the Job handle and output buffer remain valid for this call.
            QueryInformationJobObject(
                self.handle.as_raw_handle().cast(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if queried == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(accounting.ActiveProcesses == 0)
        }
    }

    fn request_termination(&self) -> std::io::Result<()> {
        let terminated = unsafe {
            // SAFETY: the Job handle remains valid for this synchronous call.
            TerminateJobObject(self.handle.as_raw_handle().cast(), 1)
        };
        if terminated == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn capture_processes(
        &self,
        processes: &mut BTreeMap<u32, OwnedHandle>,
        deadline: tokio::time::Instant,
    ) -> std::io::Result<bool> {
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "install process cleanup exceeded its deadline",
            ));
        }
        let mut list = JobProcessIds {
            assigned: 0,
            count: 0,
            ids: [0; MAX_CLEANUP_PROCESSES],
        };
        // The repr(C) buffer has the native two-DWORD header and aligned PID array.
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw_handle().cast(),
                JobObjectBasicProcessIdList,
                (&mut list as *mut JobProcessIds).cast(),
                size_of::<JobProcessIds>() as u32,
                null_mut(),
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                std::io::Error::other("install process tree exceeds the cleanup process limit")
            } else {
                error
            });
        }
        if list.assigned > list.count || list.count as usize > MAX_CLEANUP_PROCESSES {
            return Err(std::io::Error::other(
                "install process tree snapshot is incomplete",
            ));
        }
        let mut added = false;
        for pid in &list.ids[..list.count as usize] {
            if tokio::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "install process cleanup exceeded its deadline",
                ));
            }
            let pid = u32::try_from(*pid).map_err(|_| {
                std::io::Error::other("install process tree returned an invalid PID")
            })?;
            if processes.contains_key(&pid) {
                continue;
            }
            if processes.len() == MAX_CLEANUP_PROCESSES {
                return Err(std::io::Error::other(
                    "install process tree exceeds the cleanup process limit",
                ));
            }
            const SYNCHRONIZE: u32 = 0x00100000;
            let raw =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
            if raw.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    continue;
                }
                return Err(error);
            }
            let process = unsafe { OwnedHandle::from_raw_handle(raw) };
            let mut member = 0;
            // Check the opened object, not only its reusable numeric PID.
            if unsafe {
                IsProcessInJob(
                    process.as_raw_handle().cast(),
                    self.handle.as_raw_handle().cast(),
                    &mut member,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if member != 0 {
                processes.insert(pid, process);
                added = true;
            }
        }
        Ok(added)
    }

    fn observe_processes(&self, deadline: tokio::time::Instant) -> std::io::Result<bool> {
        let mut observed = self
            .observed
            .lock()
            .map_err(|_| std::io::Error::other("install process observation is unavailable"))?;
        self.capture_processes(&mut observed, deadline)
    }

    fn observed_processes_exited(&self, deadline: tokio::time::Instant) -> std::io::Result<bool> {
        let observed = self
            .observed
            .lock()
            .map_err(|_| std::io::Error::other("install process observation is unavailable"))?;
        let mut exited = true;
        for process in observed.values() {
            if tokio::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "install process cleanup exceeded its deadline",
                ));
            }
            match unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => exited = false,
                _ => return Err(std::io::Error::last_os_error()),
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "install process cleanup exceeded its deadline",
            ));
        }
        Ok(exited)
    }
}

#[cfg(windows)]
fn resume_suspended_process_threads(pid: u32) -> std::io::Result<()> {
    let snapshot_raw = unsafe {
        // SAFETY: a system-wide thread snapshot does not dereference caller memory.
        CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    };
    if snapshot_raw == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let snapshot = unsafe {
        // SAFETY: CreateToolhelp32Snapshot returned one owned snapshot handle.
        OwnedHandle::from_raw_handle(snapshot_raw)
    };
    let mut entry: THREADENTRY32 = unsafe {
        // SAFETY: THREADENTRY32 is valid when zeroed and dwSize is assigned.
        zeroed()
    };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    let mut found = false;
    let mut has_entry = unsafe {
        // SAFETY: snapshot and entry are valid for enumeration.
        Thread32First(snapshot.as_raw_handle().cast(), &mut entry)
    } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            let thread_raw = unsafe {
                // SAFETY: the thread id came from the current snapshot entry.
                OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID)
            };
            if thread_raw.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let thread = unsafe {
                // SAFETY: OpenThread returned one owned thread handle.
                OwnedHandle::from_raw_handle(thread_raw)
            };
            let previous_count = unsafe {
                // SAFETY: the opened thread belongs to the suspended child.
                ResumeThread(thread.as_raw_handle().cast())
            };
            if previous_count == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            found = true;
        }
        has_entry = unsafe {
            // SAFETY: snapshot and entry remain valid for enumeration.
            Thread32Next(snapshot.as_raw_handle().cast(), &mut entry)
        } != 0;
    }
    if !found {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "suspended child primary thread was not found",
        ));
    }
    Ok(())
}

pub(super) async fn spawn_managed_command(
    command: &mut Command,
) -> Result<(Child, ChildProcessGuard), SteamCmdError> {
    spawn_managed_command_classified(command)
        .await
        .map_err(|error| match error {
            ManagedCommandSpawnError::NotStarted(source) => SteamCmdError::SpawnCommand { source },
            ManagedCommandSpawnError::ProcessManagement(error) => error,
        })
}

pub(super) enum ManagedCommandSpawnError {
    // Only Command::spawn returning Err establishes that no child was created.
    NotStarted(std::io::Error),
    ProcessManagement(SteamCmdError),
}

pub(super) async fn spawn_managed_command_classified(
    command: &mut Command,
) -> Result<(Child, ChildProcessGuard), ManagedCommandSpawnError> {
    command.kill_on_drop(true);

    #[cfg(windows)]
    {
        let job = WindowsJob::new().map_err(|source| {
            ManagedCommandSpawnError::ProcessManagement(SteamCmdError::SpawnCommand { source })
        })?;
        command.creation_flags(super::hidden_child_creation_flags() | CREATE_SUSPENDED);
        let mut child = command
            .spawn()
            .map_err(ManagedCommandSpawnError::NotStarted)?;
        let mut guard = ChildProcessGuard { job: Some(job) };
        let Some(pid) = child.id() else {
            return Err(reap_failed_command_start(
                &mut child,
                &mut guard,
                std::io::Error::other("spawned child did not expose a process id"),
            )
            .await);
        };
        if let Some(job) = guard.job.as_ref()
            && let Err(source) = job.assign_and_resume(pid)
        {
            return Err(reap_failed_command_start(&mut child, &mut guard, source).await);
        }
        Ok((child, guard))
    }

    #[cfg(not(windows))]
    {
        let child = command
            .spawn()
            .map_err(ManagedCommandSpawnError::NotStarted)?;
        Ok((child, ChildProcessGuard {}))
    }
}

#[cfg(windows)]
async fn reap_failed_command_start(
    child: &mut Child,
    guard: &mut ChildProcessGuard,
    source: std::io::Error,
) -> ManagedCommandSpawnError {
    let error = match terminate_and_reap_preparation_tree(child, guard).await {
        Ok(()) => SteamCmdError::SpawnCommand { source },
        Err(cleanup) => SteamCmdError::InstallProcessCleanupFailed {
            operation: String::from("managed command startup"),
            detail: format!(
                "Command startup failed: {source}; owned process cleanup failed: {cleanup}"
            ),
        },
    };
    ManagedCommandSpawnError::ProcessManagement(error)
}

pub(super) async fn run_command_capture(
    mut command: Command,
    deadline: InstallDeadline,
) -> Result<std::process::Output, SteamCmdError> {
    deadline.check_cancelled()?;
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let (child, process_guard) = spawn_managed_command(&mut command).await?;
    capture_spawned_command(child, process_guard, deadline).await
}

async fn capture_spawned_command(
    mut child: Child,
    mut process_guard: ChildProcessGuard,
    deadline: InstallDeadline,
) -> Result<std::process::Output, SteamCmdError> {
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
            return Err(SteamCmdError::SpawnCommand {
                source: std::io::Error::other("child stdout pipe unavailable"),
            });
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_and_reap_child(&mut child, &mut process_guard, deadline).await?;
            return Err(SteamCmdError::SpawnCommand {
                source: std::io::Error::other("child stderr pipe unavailable"),
            });
        }
    };
    let mut stdout_reader = spawn_output_collector(stdout);
    let mut stderr_reader = spawn_output_collector(stderr);

    let result = {
        let operation = async {
            let status = deadline
                .run(child.wait())
                .await
                .map_err(|InstallDeadlineElapsed| operation_timeout(deadline))?
                .map_err(|source| SteamCmdError::SpawnCommand { source })?;
            let stdout = collect_output(&mut stdout_reader, deadline).await?;
            let stderr = collect_output(&mut stderr_reader, deadline).await?;
            process_guard.wait_until_empty(deadline).await?;
            Ok(std::process::Output {
                status,
                stdout,
                stderr,
            })
        };
        #[cfg(not(windows))]
        {
            operation.await
        }
        #[cfg(windows)]
        {
            tokio::pin!(operation);
            // Retain observed descendants while they are alive. Waiting until the
            // output pipes close can be too late to recover their process objects.
            let mut observations = tokio::time::interval(std::time::Duration::from_millis(25));
            observations.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = observations.tick() => {
                        if let Some(job) = process_guard.job.as_ref()
                            && let Err(source) = job.observe_processes(deadline.expires_at())
                        {
                            break Err(if tokio::time::Instant::now() >= deadline.expires_at() {
                                operation_timeout(deadline)
                            } else {
                                SteamCmdError::SpawnCommand { source }
                            });
                        }
                    }
                    result = &mut operation => break result,
                }
            }
        }
    };
    if result.is_err() {
        let cleanup = terminate_and_reap_child(&mut child, &mut process_guard, deadline).await;
        abort_output_collectors(&mut stdout_reader, &mut stderr_reader).await;
        cleanup?;
    }
    result
}

fn spawn_output_collector<R>(reader: R) -> AbortOnDropTask<std::io::Result<Vec<u8>>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    AbortOnDropTask::new(tokio::spawn(collect_bounded_output(reader)))
}

async fn collect_bounded_output<R>(mut reader: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = BoundedOutputTail::new();
    let mut chunk = [0_u8; OUTPUT_READ_CHUNK_BYTES];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(output.into_bytes());
        }
        output.push(&chunk[..read]);
    }
}

async fn collect_output(
    reader: &mut AbortOnDropTask<std::io::Result<Vec<u8>>>,
    deadline: InstallDeadline,
) -> Result<Vec<u8>, SteamCmdError> {
    match deadline.run(reader.join()).await {
        Ok(Ok(Ok(output))) => Ok(output),
        Ok(Ok(Err(source))) => Err(SteamCmdError::SpawnCommand { source }),
        Ok(Err(source)) => Err(SteamCmdError::SpawnCommand {
            source: std::io::Error::other(format!("child output reader failed: {source}")),
        }),
        Err(InstallDeadlineElapsed) => {
            reader.abort_and_join().await;
            Err(operation_timeout(deadline))
        }
    }
}

async fn abort_output_collectors(
    stdout: &mut AbortOnDropTask<std::io::Result<Vec<u8>>>,
    stderr: &mut AbortOnDropTask<std::io::Result<Vec<u8>>>,
) {
    stdout.abort();
    stderr.abort();
    stdout.abort_and_join().await;
    stderr.abort_and_join().await;
}

pub(super) async fn terminate_and_reap_child(
    child: &mut Child,
    process_guard: &mut ChildProcessGuard,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    terminate_and_reap_preparation_tree(child, process_guard)
        .await
        .map_err(|error| SteamCmdError::InstallProcessCleanupFailed {
            operation: deadline.operation().to_owned(),
            detail: error.to_string(),
        })
}

pub(super) async fn terminate_and_reap_preparation_tree(
    child: &mut Child,
    process_guard: &mut ChildProcessGuard,
) -> Result<(), SteamCmdError> {
    // Keep the Job handle until every descendant has exited. A self-update
    // verification must not race a still-terminating updater over the same files.
    let cleanup_deadline = tokio::time::Instant::now() + CHILD_REAP_TIMEOUT;
    #[cfg(windows)]
    let captured = process_guard
        .job
        .as_ref()
        .map(|job| job.observe_processes(cleanup_deadline));
    #[cfg(windows)]
    if let Some(job) = process_guard.job.as_ref()
        && let Err(source) = job.request_termination()
        && !job.is_empty().unwrap_or(false)
    {
        return Err(SteamCmdError::SpawnCommand { source });
    }
    #[cfg(windows)]
    if let Some(Err(source)) = captured {
        return Err(SteamCmdError::SpawnCommand { source });
    }
    // TerminateJobObject may have completed between try_wait and kill.
    if child
        .try_wait()
        .map_err(|source| SteamCmdError::SpawnCommand { source })?
        .is_none()
        && let Err(source) = child.start_kill()
        && child.try_wait().ok().flatten().is_none()
    {
        return Err(SteamCmdError::SpawnCommand { source });
    }
    // Cleanup must finish even when the operation token is already cancelled.
    // Never route these waits through the cancellable operation deadline.
    tokio::time::timeout_at(cleanup_deadline, async {
        child.wait().await?;
        #[cfg(windows)]
        if let Some(job) = process_guard.job.as_ref() {
            loop {
                let added = job.observe_processes(cleanup_deadline)?;
                let empty = job.is_empty()?;
                if added && !empty {
                    job.request_termination()?;
                }
                // Job accounting can reach zero before the last process object
                // signals exit. Keep verified handles until both facts agree.
                let all_exited = job.observed_processes_exited(cleanup_deadline)?;
                if empty && all_exited {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        process_guard.wait_for_empty(cleanup_deadline).await
    })
    .await
    .map_err(|_| SteamCmdError::OperationTimedOut {
        operation: "owned process cleanup",
        timeout_seconds: CHILD_REAP_TIMEOUT.as_secs(),
    })?
    .map_err(|source| SteamCmdError::SpawnCommand { source })
}

pub(super) async fn run_powershell(
    script: &str,
    working_directory: Option<&Path>,
    deadline: InstallDeadline,
) -> Result<std::process::Output, SteamCmdError> {
    let mut command = Command::new("powershell");
    apply_no_window(&mut command);
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script);

    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }

    run_command_capture(command, deadline).await
}

#[cfg(test)]
#[path = "install_process_cancellation_tests.rs"]
mod cancellation_tests;

#[cfg(all(test, windows))]
#[path = "install_process_deadline_tests.rs"]
mod deadline_tests;

#[cfg(test)]
mod output_tests {
    use std::future::pending;

    use tokio::io::AsyncWriteExt;
    use tokio::sync::oneshot;

    use super::*;

    #[tokio::test]
    async fn output_collector_drains_input_but_retains_only_a_bounded_tail() {
        let (mut writer, reader) = tokio::io::duplex(4 * 1024);
        let mut collector = spawn_output_collector(reader);
        let mut input = vec![b'a'; COMMAND_OUTPUT_PAYLOAD_LIMIT_BYTES];
        input.extend(std::iter::repeat_n(b'b', OUTPUT_READ_CHUNK_BYTES * 2));

        writer
            .write_all(&input)
            .await
            .expect("write oversized output");
        writer.shutdown().await.expect("close output writer");
        let captured = collector
            .join()
            .await
            .expect("join output collector")
            .expect("read output");

        assert!(captured.len() <= COMMAND_OUTPUT_CAPTURE_LIMIT_BYTES);
        assert!(captured.starts_with(COMMAND_OUTPUT_TRUNCATED_PREFIX));
        let expected_tail = vec![b'b'; OUTPUT_READ_CHUNK_BYTES * 2];
        assert!(captured.ends_with(&expected_tail));
    }

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn owned_reader_task_aborts_when_its_parent_future_is_dropped() {
        let (started_sender, started_receiver) = oneshot::channel();
        let (dropped_sender, dropped_receiver) = oneshot::channel();
        let task = AbortOnDropTask::new(tokio::spawn(async move {
            let _drop_signal = DropSignal(Some(dropped_sender));
            let _ = started_sender.send(());
            pending::<()>().await;
        }));

        started_receiver.await.expect("reader task starts");
        assert!(!task.is_finished());
        drop(task);
        tokio::time::timeout(CHILD_REAP_TIMEOUT, dropped_receiver)
            .await
            .expect("reader task cancellation deadline")
            .expect("reader task releases its resources");
    }
}
