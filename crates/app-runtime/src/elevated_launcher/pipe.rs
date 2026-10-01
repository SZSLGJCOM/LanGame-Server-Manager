use std::io;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS, SetNamedPipeHandleState,
};

use crate::OwnedWindowsHandle;

pub(super) const MAX_FRAME: usize = 65536;
pub(super) const POLL: Duration = Duration::from_millis(20);

#[derive(Debug)]
pub(super) struct Channel(OwnedWindowsHandle);

// A channel has one owner, no pending overlapped operations, and no thread affinity.
unsafe impl Send for Channel {}

pub(super) fn nonce() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "launcher nonce generation failed: {status}"
        )));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn pipe_name(nonce: &str) -> io::Result<Vec<u16>> {
    if nonce.len() != 48 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid launcher namespace",
        ));
    }
    Ok(crate::wide_null(&format!(
        r"\\.\pipe\LanGame.Elevated.{nonce}"
    )))
}

impl Channel {
    pub(super) fn server(nonce: &str) -> io::Result<Self> {
        let name = pipe_name(nonce)?;
        super::security::with_pipe_security(|attributes| {
            let raw = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    MAX_FRAME as u32 + 4,
                    MAX_FRAME as u32 + 4,
                    0,
                    attributes,
                )
            };
            if raw == INVALID_HANDLE_VALUE {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(OwnedWindowsHandle::new(raw)))
            }
        })
    }

    pub(super) fn connect(nonce: &str) -> io::Result<Self> {
        let name = pipe_name(nonce)?;
        // Do not allow the medium-integrity server to impersonate this client.
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let channel = Self(OwnedWindowsHandle::new(raw));
        if unsafe {
            SetNamedPipeHandleState(
                channel.0.as_raw(),
                &PIPE_NOWAIT,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(channel)
    }

    pub(super) fn accept(&self, deadline: Instant) -> io::Result<()> {
        loop {
            if unsafe { ConnectNamedPipe(self.0.as_raw(), std::ptr::null_mut()) } != 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(535) => return Ok(()),    // ERROR_PIPE_CONNECTED
                Some(536) => pause(deadline)?, // ERROR_PIPE_LISTENING
                _ => return Err(error),
            }
        }
    }

    pub(super) fn peer_pid(&self, server: bool) -> io::Result<u32> {
        let mut pid = 0;
        let result = unsafe {
            if server {
                GetNamedPipeClientProcessId(self.0.as_raw(), &mut pid)
            } else {
                GetNamedPipeServerProcessId(self.0.as_raw(), &mut pid)
            }
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(pid)
        }
    }

    pub(super) fn send(&self, value: &serde_json::Value, deadline: Instant) -> io::Result<()> {
        let payload = serde_json::to_vec(value)?;
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "launcher packet too large",
            ));
        }
        let mut bytes = (payload.len() as u32).to_le_bytes().to_vec();
        bytes.extend(payload);
        self.write_all(&bytes, deadline)
    }

    pub(super) fn receive(&self, deadline: Instant) -> io::Result<serde_json::Value> {
        let mut length = [0u8; 4];
        self.read_exact(&mut length, deadline)?;
        let length = u32::from_le_bytes(length) as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid launcher packet size",
            ));
        }
        let mut bytes = vec![0; length];
        self.read_exact(&mut bytes, deadline)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(super) fn write_all(&self, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            check_deadline(deadline)?;
            let mut written = 0;
            if unsafe {
                WriteFile(
                    self.0.as_raw(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            bytes = &bytes[written as usize..];
            if written == 0 {
                pause(deadline)?;
            }
        }
        Ok(())
    }

    fn read_exact(&self, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            check_deadline(deadline)?;
            let read = self.read_available(bytes)?;
            bytes = &mut bytes[read..];
            if read == 0 {
                pause(deadline)?;
            }
        }
        Ok(())
    }

    /// A single nonblocking native read. The helper checks its parent handle on
    /// every loop even if the authenticated parent never sends another byte.
    pub(super) fn read_available(&self, bytes: &mut [u8]) -> io::Result<usize> {
        let mut read = 0;
        if unsafe {
            ReadFile(
                self.0.as_raw(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(232) {
                return Ok(0);
            } // ERROR_NO_DATA
            return Err(error);
        }
        Ok(read as usize)
    }
}

pub(super) fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "elevated launcher deadline expired",
        ))
    } else {
        Ok(())
    }
}

fn pause(deadline: Instant) -> io::Result<()> {
    check_deadline(deadline)?;
    std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    Ok(())
}
