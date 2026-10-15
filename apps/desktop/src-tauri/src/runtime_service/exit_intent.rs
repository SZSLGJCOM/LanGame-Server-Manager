//! A helper-owned marker closes the gap before the service receives final exit.
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE,
};

use super::security;

/// Windows removes the marker when its final owner closes the native handle.
/// The exit helper keeps this owner alive until the pinned service has exited.
pub(super) struct ExitIntent {
    _event: OwnedHandle,
}

pub(super) fn mark(identity: &security::ServiceIdentity) -> io::Result<ExitIntent> {
    let sid = security::user_sid()?;
    let name = event_name(&sid, identity);
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})\0").encode_utf16().collect();
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateEventW(&attributes, 1, 1, name.as_ptr());
        let result = if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(ExitIntent {
                _event: OwnedHandle::from_raw_handle(handle),
            })
        };
        LocalFree(descriptor);
        result
    }
}

pub(super) fn is_marked(identity: &security::ServiceIdentity) -> io::Result<bool> {
    let sid = security::user_sid()?;
    let name = event_name(&sid, identity);
    let handle = unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) {
            Ok(false)
        } else {
            Err(error)
        };
    }
    // Readers release their handle immediately, so observing final exit cannot
    // preserve a stale marker or affect the lifetime of another service process.
    drop(unsafe { OwnedHandle::from_raw_handle(handle) });
    Ok(true)
}

fn event_name(sid: &str, identity: &security::ServiceIdentity) -> Vec<u16> {
    format!(
        "Local\\LanGame.Runtime.ExitIntent.{sid}.{}.{:016x}\0",
        identity.pid, identity.process.creation_time
    )
    .encode_utf16()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_scoped_to_the_process_identity_and_native_owner_lifetime() {
        let pid = std::process::id();
        let process = app_runtime::inspect_process_identity(pid)
            .expect("inspect test process")
            .expect("test process exists");
        let mut identity = security::ServiceIdentity { pid, process };
        assert!(!is_marked(&identity).expect("check initial marker"));

        let owner = mark(&identity).expect("mark test process");
        assert!(is_marked(&identity).expect("observe owned marker"));
        identity.pid = pid.wrapping_add(1);
        assert!(!is_marked(&identity).expect("check another PID"));
        identity.pid = pid;
        identity.process.creation_time = identity.process.creation_time.wrapping_add(1);
        assert!(!is_marked(&identity).expect("check reused PID with another creation time"));
        identity.process.creation_time = identity.process.creation_time.wrapping_sub(1);

        let other_owner = mark(&identity).expect("share marker ownership");
        drop(owner);
        assert!(is_marked(&identity).expect("second owner preserves marker"));
        drop(other_owner);
        assert!(!is_marked(&identity).expect("last owner releases marker"));
    }
}
