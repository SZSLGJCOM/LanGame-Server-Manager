use std::fs;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::STILL_ACTIVE;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

pub(crate) struct OwnedProcessTree {
    processes: Vec<(&'static str, OwnedHandle)>,
}

impl OwnedProcessTree {
    pub(crate) fn capture_running(root: &Path) -> Self {
        let processes = ["parent-pid", "child-pid"]
            .into_iter()
            .map(|name| {
                let published = fs::read_to_string(root.join(name)).unwrap();
                let pid: u32 = published
                    .parse()
                    .unwrap_or_else(|error| panic!("{name} published {published:?}: {error}"));
                let raw = unsafe {
                    // SAFETY: this PID was published by our fixture process.
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
                };
                assert!(
                    !raw.is_null(),
                    "{name} must be running before cancellation: {}",
                    std::io::Error::last_os_error()
                );
                let process = unsafe {
                    // SAFETY: OpenProcess returned an owned process handle.
                    OwnedHandle::from_raw_handle(raw)
                };
                assert_eq!(
                    exit_code(&process),
                    STILL_ACTIVE as u32,
                    "{name} exited before cancellation"
                );
                (name, process)
            })
            .collect();
        Self { processes }
    }

    pub(crate) fn assert_exited(&self) {
        // Handles captured while both processes were alive pin their identities;
        // looking up PIDs only after cancellation could observe PID reuse.
        for (name, process) in &self.processes {
            assert_ne!(
                exit_code(process),
                STILL_ACTIVE as u32,
                "{name} survived cancellation"
            );
        }
    }
}

fn exit_code(process: &OwnedHandle) -> u32 {
    let mut exit_code = 0;
    let queried = unsafe {
        // SAFETY: the owned handle and exit-code buffer remain valid.
        GetExitCodeProcess(process.as_raw_handle().cast(), &mut exit_code)
    };
    assert_ne!(queried, 0, "{}", std::io::Error::last_os_error());
    exit_code
}
