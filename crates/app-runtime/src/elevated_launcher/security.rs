use std::io;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
    TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Threading::OpenProcessToken;

use crate::{GetCurrentProcess, OwnedWindowsHandle, WindowsProcessHandle};

pub(super) fn user_sid(process: *mut std::ffi::c_void) -> io::Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedWindowsHandle::new(token);
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
        if needed == 0 || needed > 65536 {
            return Err(io::Error::other("invalid process token size"));
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Ok(result)
    }
}

pub(super) fn require_elevated() -> io::Result<()> {
    require_elevated_process(unsafe { GetCurrentProcess() })
}

fn require_elevated_process(process: *mut std::ffi::c_void) -> io::Result<()> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedWindowsHandle::new(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut needed = 0;
        if GetTokenInformation(
            token.as_raw(),
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of_val(&elevation) as u32,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        validate_elevation(elevation.TokenIsElevated)
    }
}

fn validate_elevation(elevated: u32) -> io::Result<()> {
    if elevated == 0 {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launcher requires the operator-approved elevated token",
        ))
    } else {
        Ok(())
    }
}

/// Both ends must be the same binary and Windows account. The parent additionally
/// pins the child PID returned by ShellExecuteEx; the helper pins parent creation.
pub(super) fn verify_peer(process: &WindowsProcessHandle) -> io::Result<()> {
    verify_handle(process.raw, process.pid)
}

pub(super) fn verify_handle(raw: *mut std::ffi::c_void, pid: u32) -> io::Result<()> {
    let peer =
        crate::query_windows_process_identity_from_handle(pid, raw).map_err(io::Error::other)?;
    let current = crate::query_windows_process_identity_from_handle(std::process::id(), unsafe {
        GetCurrentProcess()
    })
    .map_err(io::Error::other)?;
    if peer.image_path != current.image_path
        || user_sid(raw)? != user_sid(unsafe { GetCurrentProcess() })?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launcher peer identity mismatch",
        ));
    }
    Ok(())
}

pub(super) fn with_pipe_security<T>(
    create: impl FnOnce(&SECURITY_ATTRIBUTES) -> io::Result<T>,
) -> io::Result<T> {
    let sid = user_sid(unsafe { GetCurrentProcess() })?;
    // The protected DACL excludes other accounts; remote clients are also rejected
    // by CreateNamedPipe. Identification-only client SQOS prevents impersonation.
    let sddl = crate::wide_null(&format!("D:P(A;;GA;;;{sid})"));
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
        let result = create(&attributes);
        LocalFree(descriptor);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_entry_rejects_non_elevated_token_and_retains_native_errors() {
        assert_eq!(
            validate_elevation(0).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(validate_elevation(1).is_ok());
        let error = require_elevated_process(std::ptr::null_mut()).unwrap_err();
        assert!(
            error.raw_os_error().is_some(),
            "native token query error must survive"
        );
    }
}
