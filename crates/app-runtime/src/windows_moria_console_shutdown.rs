//! Release only the native console input read after the engine requests exit.
//! A cancellation receipt never substitutes for the supervisor's whole-Job exit.
use super::*;
use std::time::{Duration, Instant};

#[path = "windows_moria_console_profile.rs"]
mod profile;

const INPUT_THREAD: &str = "MorDedicatedServerConsole::InputThread";
const MAX_SNAPSHOT_THREADS: usize = 65_536;
const MAX_OWNED_THREADS: usize = 256;

pub(super) struct ShutdownRead {
    profile: profile::Profile,
    identity: ProcessIdentity,
    job: *mut c_void,
    deadline: Instant,
}

impl ShutdownRead {
    pub(super) fn prepare(
        pid: u32,
        identity: &ProcessIdentity,
        job: *mut c_void,
        deadline: Instant,
    ) -> Result<Self, RuntimeProcessError> {
        let profile = profile::Profile::open(pid, identity, deadline)?;
        if !belongs(&profile.process, job)? {
            return Err(error(pid, "Moria shutdown reader is outside the owned Job"));
        }
        Ok(Self {
            profile,
            identity: identity.clone(),
            job,
            deadline,
        })
    }

    pub(super) fn finish(self) -> Result<&'static str, RuntimeProcessError> {
        let pid = self.profile.process.pid;
        loop {
            if !self.profile.process.is_running()? {
                return Ok("native_process_exited");
            }
            self.verify_owner()?;
            if Instant::now() >= self.deadline {
                return Err(error(
                    pid,
                    "Moria engine exit was not observed before the shutdown helper deadline; no retry",
                ));
            }
            if self.profile.exit_requested()? {
                // This transition is written by RequestEngineExit, after the
                // command has reached the native game thread. Never cancel the
                // ReadConsole that might still be consuming the first exit line.
                return self.cancel_input_once();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn before_write(&self) -> Result<(), RuntimeProcessError> {
        self.verify_owner()?;
        if Instant::now() >= self.deadline || self.profile.exit_requested()? {
            return Err(error(
                self.profile.process.pid,
                "Moria shutdown state changed before input delivery",
            ));
        }
        Ok(())
    }

    fn verify_owner(&self) -> Result<(), RuntimeProcessError> {
        if !belongs(&self.profile.process, self.job)?
            || !process_identities_match(&self.identity, &self.profile.process.identity()?)
        {
            return Err(error(
                self.profile.process.pid,
                "Moria shutdown process ownership changed",
            ));
        }
        Ok(())
    }

    fn cancel_input_once(&self) -> Result<&'static str, RuntimeProcessError> {
        let pid = self.profile.process.pid;
        let selected = find_input_thread(pid, self.identity.creation_time, self.deadline)?;
        self.verify_owner()?;
        if !self.profile.process.is_running()? {
            return Ok("native_process_exited");
        }
        if !self.profile.exit_requested()? {
            return Err(error(
                pid,
                "Moria engine exit state changed before console read cancellation",
            ));
        }
        let Some(thread) = selected else {
            return Ok("input_thread_exited");
        };
        verify_thread(pid, self.identity.creation_time, thread.0)?;
        if Instant::now() >= self.deadline {
            return Err(error(
                pid,
                "Moria shutdown helper deadline expired before console read cancellation",
            ));
        }
        // THREAD_TERMINATE is the documented permission for this I/O API.
        // TerminateThread is never used. The native error branch exits its
        // input loop; all world-saving and engine-exit work remains native.
        let result = unsafe { CancelSynchronousIo(thread.0) };
        cancellation_result(result != 0, std::io::Error::last_os_error().raw_os_error()).ok_or_else(
            || {
                error(
                    pid,
                    "Moria console read cancellation failed; exit is not replayed",
                )
            },
        )
    }
}

fn cancellation_result(success: bool, code: Option<i32>) -> Option<&'static str> {
    if success {
        Some("cancel_requested")
    } else if code == Some(1168) {
        Some("no_pending_read")
    } else {
        None
    }
}

#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    tid: u32,
    owner: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

fn thread_description(thread: *mut c_void) -> Option<String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { GetThreadDescription(thread, &mut raw) } < 0 || raw.is_null() {
        return None;
    }
    let mut length = 0;
    while length < 256 && unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    let result = if length < 256 {
        String::from_utf16(unsafe { std::slice::from_raw_parts(raw, length) }).ok()
    } else {
        None
    };
    unsafe {
        LocalFree(raw.cast());
    }
    result
}

fn verify_thread(
    pid: u32,
    process_created: u64,
    thread: *mut c_void,
) -> Result<(), RuntimeProcessError> {
    let mut created = 0u64;
    let mut ended = 0u64;
    let mut kernel = 0u64;
    let mut user = 0u64;
    if unsafe { GetProcessIdOfThread(thread) } != pid
        || thread_description(thread).as_deref() != Some(INPUT_THREAD)
        || unsafe { GetThreadTimes(thread, &mut created, &mut ended, &mut kernel, &mut user) } == 0
        || created < process_created
    {
        return Err(error(pid, "Moria input thread identity changed"));
    }
    Ok(())
}

fn find_input_thread(
    pid: u32,
    process_created: u64,
    deadline: Instant,
) -> Result<Option<Handle>, RuntimeProcessError> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(0x04, 0) });
    if snapshot.0.is_null() || snapshot.0 as isize == -1 {
        return Err(error(pid, "Moria input thread snapshot unavailable"));
    }
    let mut entry = ThreadEntry {
        size: std::mem::size_of::<ThreadEntry>() as u32,
        usage: 0,
        tid: 0,
        owner: 0,
        base_priority: 0,
        delta_priority: 0,
        flags: 0,
    };
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        return Err(error(pid, "Moria input thread snapshot could not be read"));
    }
    let mut total = 0;
    let mut owned = 0;
    let mut selected = None;
    loop {
        total += 1;
        if total > MAX_SNAPSHOT_THREADS || Instant::now() >= deadline {
            return Err(error(pid, "Moria input thread search exceeded its bound"));
        }
        if entry.owner == pid {
            owned += 1;
            if owned > MAX_OWNED_THREADS {
                return Err(error(pid, "Moria owned thread count exceeds its bound"));
            }
            // Enumerate with query-only handles; acquire cancellation rights
            // only for the one native input thread selected by its exact name.
            let thread = Handle(unsafe { OpenThread(0x0800, 0, entry.tid) });
            if !thread.0.is_null() {
                if thread_description(thread.0).as_deref() == Some(INPUT_THREAD) {
                    verify_thread(pid, process_created, thread.0)?;
                    let mut exit = 0;
                    if unsafe { GetExitCodeThread(thread.0, &mut exit) } == 0 {
                        return Err(error(pid, "Moria input thread exit state unavailable"));
                    }
                    if exit == 259 {
                        if selected.is_some() {
                            return Err(error(pid, "Moria native input thread is ambiguous"));
                        }
                        let cancellation = Handle(unsafe { OpenThread(0x0801, 0, entry.tid) });
                        if cancellation.0.is_null() {
                            return Err(error(
                                pid,
                                "Moria console read cancellation rights unavailable",
                            ));
                        }
                        verify_thread(pid, process_created, cancellation.0)?;
                        selected = Some(cancellation);
                    }
                }
            } else if std::io::Error::last_os_error().raw_os_error() != Some(87) {
                return Err(error(
                    pid,
                    "Moria owned input thread could not be inspected",
                ));
            }
        }
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(18) {
                return Err(error(pid, "Moria input thread snapshot ended unexpectedly"));
            }
            break;
        }
    }
    Ok(selected)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> *mut c_void;
    fn GetThreadDescription(thread: *mut c_void, description: *mut *mut u16) -> i32;
    fn GetProcessIdOfThread(thread: *mut c_void) -> u32;
    fn GetThreadTimes(
        thread: *mut c_void,
        created: *mut u64,
        exited: *mut u64,
        kernel: *mut u64,
        user: *mut u64,
    ) -> i32;
    fn GetExitCodeThread(thread: *mut c_void, exit: *mut u32) -> i32;
    fn CancelSynchronousIo(thread: *mut c_void) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pending_io_does_not_claim_that_a_read_was_cancelled() {
        assert_eq!(cancellation_result(true, None), Some("cancel_requested"));
        assert_eq!(
            cancellation_result(false, Some(1168)),
            Some("no_pending_read")
        );
        assert_eq!(cancellation_result(false, Some(5)), None);
        assert_eq!(cancellation_result(false, None), None);
    }
}
