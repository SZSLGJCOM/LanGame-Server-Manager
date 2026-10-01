use std::io;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(super) const MAX_WORK_REQUESTS: usize = 32;
pub(super) const AUXILIARY_CONNECTION_RESERVE: usize = 2;
pub(super) const MAX_CONNECTIONS: usize = MAX_WORK_REQUESTS + AUXILIARY_CONNECTION_RESERVE;
// Every accepted connection retains its pipe while accept prepares the next
// secured listener. That additional handle must fit even at full admission.
pub(super) const MAX_PIPE_INSTANCES: usize = MAX_CONNECTIONS + 1;
pub(super) const MAX_CONTROL_CONNECTIONS: usize = 2;

pub(super) fn is_control_command(command: &str) -> bool {
    matches!(
        command,
        "runtime_service_shutdown" | "runtime_service_tray_exit" | "runtime_service_status"
    )
}

pub(super) struct ServiceIdentity {
    pub(super) pid: u32,
    pub(super) process: app_core::ProcessIdentity,
}

#[derive(Clone)]
pub(super) struct Endpoint {
    sid: String,
    name: String,
    control: bool,
}

impl Endpoint {
    pub(super) fn current() -> io::Result<Self> {
        let sid = user_sid()?;
        let name = format!(r"\\.\pipe\LanGame.Runtime.{sid}");
        Ok(Self {
            sid,
            name,
            control: false,
        })
    }

    pub(super) fn control(&self) -> Self {
        Self {
            sid: self.sid.clone(),
            name: if self.control {
                self.name.clone()
            } else {
                format!("{}.Control", self.name)
            },
            control: true,
        }
    }

    #[cfg(any(test, feature = "desktop-reliability"))]
    pub(super) fn isolated(nonce: &str) -> io::Result<Self> {
        if nonce.len() < 32
            || nonce.len() > 128
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid isolated runtime namespace",
            ));
        }
        Ok(Self {
            sid: user_sid()?,
            name: format!(r"\\.\pipe\LanGame.Runtime.Fixture.{nonce}"),
            control: false,
        })
    }
}

/// No token is written to disk or exposed in process arguments. The pipe ACL
/// grants access only to the launching Windows account, and rejects network clients.
pub(super) fn user_sid() -> io::Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            if needed == 0 || needed > 65536 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token,
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
        })();
        CloseHandle(token);
        result
    }
}

pub(super) fn create_pipe(endpoint: &Endpoint, first: bool) -> io::Result<NamedPipeServer> {
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})\0", endpoint.sid)
        .encode_utf16()
        .collect();
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
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(if endpoint.control {
                MAX_CONTROL_CONNECTIONS + 1
            } else {
                MAX_PIPE_INSTANCES
            })
            .create_with_security_attributes_raw(
                &endpoint.name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(descriptor);
        result
    }
}

/// Mio cancels pending I/O on drop, but its completion owns the OS handle until
/// the reactor drains IOCP. Admission can therefore free a slot just before
/// Windows frees that pipe instance. Never retry the first-name ownership claim.
pub(super) async fn create_replacement_pipe(endpoint: &Endpoint) -> io::Result<NamedPipeServer> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match create_pipe(endpoint, false) {
            Err(error)
                if error.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline =>
            {
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

/// Wait only before sending: the accept loop may still be preparing its next
/// listener. Dropping this future cancels the wait without a background task.
pub(super) async fn connect_when_available(endpoint: &Endpoint) -> io::Result<NamedPipeClient> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    connect_when_available_until(endpoint, deadline).await
}

/// A final-exit owner may spend its remaining budget waiting for a busy control
/// pipe. No request bytes have been written here; identity and other errors fail
/// immediately, and the caller retains ownership of the original deadline.
pub(super) async fn connect_when_available_until(
    endpoint: &Endpoint,
    deadline: tokio::time::Instant,
) -> io::Result<NamedPipeClient> {
    let mut backoff_ms = 5u64;
    loop {
        match connect_verified(endpoint) {
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(error);
                }
                // Spread competing clients without busy-spinning or blocking a worker.
                let half = backoff_ms / 2;
                let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]) % (half + 1);
                let next = tokio::time::Instant::now() + Duration::from_millis(half + jitter);
                tokio::time::sleep_until(next.min(deadline)).await;
                if tokio::time::Instant::now() >= deadline {
                    return Err(error);
                }
                backoff_ms = (backoff_ms * 2).min(50);
            }
            result => return result,
        }
    }
}

pub(super) fn connect_verified(endpoint: &Endpoint) -> io::Result<NamedPipeClient> {
    // Identification prevents an impersonating pipe server from borrowing the client token.
    let pipe = ClientOptions::new()
        .security_qos_flags(0x0001_0000)
        .open(&endpoint.name)?;
    service_identity(&pipe)?;
    Ok(pipe)
}

pub(super) fn service_identity(pipe: &NamedPipeClient) -> io::Result<ServiceIdentity> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let identity = app_runtime::inspect_process_identity(pid)
        .map_err(|error| io::Error::other(error.to_string()))?
        .ok_or_else(|| io::Error::other("Runtime service process exited"))?;
    let expected = dunce::canonicalize(std::env::current_exe()?)?;
    let actual = dunce::canonicalize(&identity.image_path)?;
    if !expected
        .to_string_lossy()
        .eq_ignore_ascii_case(&actual.to_string_lossy())
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "A different LanGame executable owns the runtime service. Stop it from its original installation before switching builds.",
        ));
    }
    Ok(ServiceIdentity {
        pid,
        process: identity,
    })
}

pub(super) fn verify_service_identity(
    pipe: &NamedPipeClient,
    expected: &ServiceIdentity,
) -> io::Result<()> {
    let actual = service_identity(pipe)?;
    if expected.pid != actual.pid
        || !app_runtime::process_identities_match(&expected.process, &actual.process)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The connected runtime service was replaced; no operation was sent",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;
