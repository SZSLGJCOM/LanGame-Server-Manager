//! Observe the identified console reader after a fresh native exit response.
//! The caller retains the Shipping process/Job handles and rechecks ownership.
//! Pending I/O never establishes a completed read, save, or Job exit.

use super::{Handle, RuntimeProcessError, error};
use std::ffi::c_void;
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME, LocalFree, STILL_ACTIVE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeThread, GetProcessIdOfThread, GetThreadDescription, GetThreadIOPendingFlag,
    GetThreadId, GetThreadTimes, OpenThread, THREAD_QUERY_INFORMATION,
    THREAD_QUERY_LIMITED_INFORMATION, THREAD_SYNCHRONIZE, WaitForSingleObject,
};

const INPUT_THREAD: &str = "MorDedicatedServerConsole::InputThread";
const MAX_SNAPSHOT_THREADS: usize = 65_536;
const MAX_OWNED_THREADS: usize = 256;

pub(super) struct InputReader {
    handle: Handle,
    pid: u32,
    tid: u32,
    created: u64,
    deadline: Instant,
}

impl InputReader {
    pub(super) fn find(
        pid: u32,
        process_created: u64,
        deadline: Instant,
    ) -> Result<Option<Self>, RuntimeProcessError> {
        if pid == 0 || process_created == 0 {
            return Err(error(pid, "Moria input reader requires a process identity"));
        }
        check_deadline(pid, deadline)?;
        let raw = unsafe { crate::CreateToolhelp32Snapshot(0x04, 0) };
        if raw.is_null() || raw as isize == -1 {
            return Err(os_error(pid, "Moria input thread snapshot unavailable"));
        }
        let snapshot = Handle(raw);
        let mut entry = ThreadEntry::default();
        if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
            let source = std::io::Error::last_os_error();
            return if source.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                Ok(None)
            } else {
                Err(error(
                    pid,
                    format!("Moria thread snapshot could not be read: {source}"),
                ))
            };
        }
        let mut total = 0;
        let mut owned = 0;
        let mut selected = None;
        loop {
            check_deadline(pid, deadline)?;
            total += 1;
            if total > MAX_SNAPSHOT_THREADS || entry.size < 16 {
                return Err(error(
                    pid,
                    "Moria thread snapshot exceeds its inspection bound",
                ));
            }
            if entry.owner == pid {
                owned += 1;
                if owned > MAX_OWNED_THREADS {
                    return Err(error(pid, "Moria owned thread count exceeds its bound"));
                }
                if let Some(candidate) = inspect_candidate(pid, process_created, entry.tid)? {
                    if selected.is_some() {
                        return Err(error(pid, "Moria native input thread is ambiguous"));
                    }
                    selected = Some((entry.tid, candidate));
                }
            }
            entry.size = std::mem::size_of::<ThreadEntry>() as u32;
            if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
                let source = std::io::Error::last_os_error();
                if source.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                    return Err(error(
                        pid,
                        format!("Moria thread snapshot ended unexpectedly: {source}"),
                    ));
                }
                break;
            }
        }
        let Some((tid, (query, created))) = selected else {
            return Ok(None);
        };
        check_deadline(pid, deadline)?;
        // Enumeration holds limited query handles. Pending-I/O query and wait
        // access are acquired only for the unique reader in the bounded snapshot.
        let access =
            THREAD_QUERY_INFORMATION | THREAD_QUERY_LIMITED_INFORMATION | THREAD_SYNCHRONIZE;
        let raw = unsafe { OpenThread(access, 0, tid) };
        if raw.is_null() {
            let source = std::io::Error::last_os_error();
            if !thread_active(pid, query.0)? {
                return Ok(None);
            }
            return Err(error(
                pid,
                format!("Moria input reader access unavailable: {source}"),
            ));
        }
        let reader = Self {
            handle: Handle(raw),
            pid,
            tid,
            created,
            deadline,
        };
        if reader.running()? {
            Ok(Some(reader))
        } else {
            Ok(None)
        }
    }

    pub(super) fn pending(&self) -> Result<bool, RuntimeProcessError> {
        check_deadline(self.pid, self.deadline)?;
        if !self.running()? {
            return Ok(false);
        }
        let mut pending = 0;
        if unsafe { GetThreadIOPendingFlag(self.handle.0, &mut pending) } == 0 {
            let source = std::io::Error::last_os_error();
            if !self.running()? {
                return Ok(false);
            }
            return Err(error(
                self.pid,
                format!("Moria input reader I/O state unavailable: {source}"),
            ));
        }
        // This is a racy observation of any thread I/O, not proof of ReadConsole
        // completion. A visible prompt can still precede the following read.
        Ok(pending != 0 && self.running()?)
    }

    pub(super) fn running(&self) -> Result<bool, RuntimeProcessError> {
        match unsafe { WaitForSingleObject(self.handle.0, 0) } {
            WAIT_OBJECT_0 => return Ok(false),
            WAIT_TIMEOUT => {}
            _ => {
                return Err(os_error(
                    self.pid,
                    "Moria input thread wait state unavailable",
                ));
            }
        }
        if !thread_active(self.pid, self.handle.0)? {
            return Ok(false);
        }
        if unsafe { GetThreadId(self.handle.0) } != self.tid
            || unsafe { GetProcessIdOfThread(self.handle.0) } != self.pid
            || thread_creation(self.pid, self.handle.0)? != self.created
            || !named_reader(self.pid, self.handle.0)?
        {
            return Err(error(self.pid, "Moria input thread identity changed"));
        }
        match unsafe { WaitForSingleObject(self.handle.0, 0) } {
            WAIT_OBJECT_0 => Ok(false),
            WAIT_TIMEOUT => Ok(true),
            _ => Err(os_error(
                self.pid,
                "Moria input thread wait state unavailable",
            )),
        }
    }
}

fn inspect_candidate(
    pid: u32,
    process_created: u64,
    tid: u32,
) -> Result<Option<(Handle, u64)>, RuntimeProcessError> {
    let raw = unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, tid) };
    if raw.is_null() {
        let source = std::io::Error::last_os_error();
        return if source.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(None) // The snapshot thread ended before it could be pinned.
        } else {
            Err(error(
                pid,
                format!("Moria owned thread could not be inspected: {source}"),
            ))
        };
    }
    let handle = Handle(raw);
    if !thread_active(pid, raw)? || !named_reader(pid, raw)? {
        return Ok(None);
    }
    let created = thread_creation(pid, raw)?;
    if unsafe { GetThreadId(raw) } != tid
        || unsafe { GetProcessIdOfThread(raw) } != pid
        || created == 0
        || created < process_created
    {
        return Err(error(
            pid,
            "Moria input thread does not match its process generation",
        ));
    }
    Ok(thread_active(pid, raw)?.then_some((handle, created)))
}

fn thread_active(pid: u32, handle: *mut c_void) -> Result<bool, RuntimeProcessError> {
    let mut exit = 0;
    if unsafe { GetExitCodeThread(handle, &mut exit) } == 0 {
        return Err(os_error(pid, "Moria input thread exit state unavailable"));
    }
    Ok(exit == STILL_ACTIVE as u32)
}

fn thread_creation(pid: u32, handle: *mut c_void) -> Result<u64, RuntimeProcessError> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetThreadTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(os_error(
            pid,
            "Moria input thread creation time unavailable",
        ));
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

fn named_reader(pid: u32, handle: *mut c_void) -> Result<bool, RuntimeProcessError> {
    let mut raw = std::ptr::null_mut();
    let result = unsafe { GetThreadDescription(handle, &mut raw) };
    if result < 0 || raw.is_null() {
        return Err(error(
            pid,
            format!("Moria thread description unavailable: HRESULT {result:#x}"),
        ));
    }
    // Compare only the fixed ASCII name and its terminator, never arbitrary
    // thread text. GetThreadDescription owns a terminated LocalAlloc buffer.
    let matches = INPUT_THREAD
        .bytes()
        .enumerate()
        .all(|(index, byte)| (unsafe { *raw.add(index) }) == u16::from(byte))
        && unsafe { *raw.add(INPUT_THREAD.len()) } == 0;
    unsafe {
        LocalFree(raw.cast());
    }
    Ok(matches)
}

fn check_deadline(pid: u32, deadline: Instant) -> Result<(), RuntimeProcessError> {
    if Instant::now() >= deadline {
        Err(error(
            pid,
            "Moria input reader deadline expired; exit is not replayed",
        ))
    } else {
        Ok(())
    }
}

fn os_error(pid: u32, context: &str) -> RuntimeProcessError {
    error(
        pid,
        format!("{context}: {}", std::io::Error::last_os_error()),
    )
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

impl Default for ThreadEntry {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            usage: 0,
            tid: 0,
            owner: 0,
            base_priority: 0,
            delta_priority: 0,
            flags: 0,
        }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadDescription};

    struct NamedThread {
        release: Option<mpsc::Sender<()>>,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl NamedThread {
        fn start() -> Self {
            let (ready, readiness) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let join = std::thread::spawn(move || {
                let name: Vec<u16> = INPUT_THREAD.encode_utf16().chain([0]).collect();
                let result = unsafe { SetThreadDescription(GetCurrentThread(), name.as_ptr()) };
                ready.send(result).unwrap();
                let _ = released.recv_timeout(Duration::from_secs(20));
            });
            let fixture = Self {
                release: Some(release),
                join: Some(join),
            };
            assert!(readiness.recv_timeout(Duration::from_secs(5)).unwrap() >= 0);
            fixture
        }
    }

    impl Drop for NamedThread {
        fn drop(&mut self) {
            self.release.take();
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    #[test]
    fn input_reader_rejects_ambiguity_and_observes_a_pinned_thread_exit() {
        let pid = std::process::id();
        let process = crate::WindowsProcessHandle::open(pid, 0).unwrap().unwrap();
        let created = process.identity().unwrap().creation_time;
        let deadline = Instant::now() + Duration::from_secs(15);
        let first = NamedThread::start();
        let second = NamedThread::start();
        let ambiguous = InputReader::find(pid, created, deadline);
        assert!(ambiguous.is_err_and(|error| error.to_string().contains("ambiguous")));
        drop(second);
        assert!(InputReader::find(pid, u64::MAX, deadline).is_err());
        let reader = InputReader::find(pid, created, deadline).unwrap().unwrap();
        assert!(!reader.pending().unwrap());
        drop(first);
        assert!(!reader.pending().unwrap());
        assert!(InputReader::find(pid, created, deadline).unwrap().is_none());
    }
}
