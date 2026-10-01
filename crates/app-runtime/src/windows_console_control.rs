use super::*;

static WINDOWS_CONSOLE_CTRL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn request_windows_console_ctrl_c(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    with_windows_console_ctrl_lock(|| {
        verify_current_process_identity(pid, expected_identity)?;
        let result = request_windows_console_ctrl_c_with_control_and_verifier(
            &NativeWindowsConsoleControl,
            pid,
            Duration::from_millis(250),
            || verify_console_ownership(pid, expected_identity, pid),
        );
        let Err(error) = result else {
            return Ok(());
        };
        if !console_is_unavailable(&error) {
            return Err(error);
        }
        // GUI bootstrappers can remain alive without a console while their
        // workload owns one. Only a failed attach permits this search: after
        // signalling or an ownership rejection, never broadcast a second time.
        let owned = collect_windows_process_tree(pid, expected_identity)?;
        if owned.len() > 4096 {
            return Err(error);
        }
        for process in owned.iter().filter(|process| process.process_id != pid) {
            let Some(identity) = process.identity.as_ref() else {
                continue;
            };
            verify_current_process_identity(process.process_id, identity)?;
            let result = request_windows_console_ctrl_c_with_control_and_verifier(
                &NativeWindowsConsoleControl,
                process.process_id,
                Duration::from_millis(250),
                || {
                    verify_current_process_identity(process.process_id, identity)?;
                    verify_console_ownership(pid, expected_identity, process.process_id)
                },
            );
            match result {
                Err(error) if console_is_unavailable(&error) => continue,
                result => return result,
            }
        }
        Err(error)
    })
}

fn console_is_unavailable(error: &RuntimeProcessError) -> bool {
    matches!(error, RuntimeProcessError::ConsoleInterruptProcess {
        operation: "AttachConsole", source, ..
    } if source.raw_os_error() == Some(6))
}

fn verify_console_ownership(
    pid: u32,
    expected_identity: &ProcessIdentity,
    console_pid: u32,
) -> Result<(), RuntimeProcessError> {
    let sender = std::process::id();
    let rejected = |message| RuntimeProcessError::ConsoleInterruptProcess {
        pid,
        operation: "console ownership verification",
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, message),
    };
    if pid == sender {
        return Err(rejected("cannot interrupt the runtime manager itself"));
    }
    let owned = collect_windows_process_tree(pid, expected_identity)?;
    let mut members = [0_u32; 4096];
    let count = unsafe { GetConsoleProcessList(members.as_mut_ptr(), members.len() as u32) };
    if count == 0 {
        return Err(RuntimeProcessError::ConsoleInterruptProcess {
            pid,
            operation: "GetConsoleProcessList",
            source: std::io::Error::last_os_error(),
        });
    }
    let count = count as usize;
    if count > members.len()
        || !members[..count].contains(&console_pid)
        || !members[..count].contains(&sender)
    {
        return Err(rejected(
            "console membership is incomplete or the target is no longer attached",
        ));
    }
    // CTRL_C_EVENT cannot target a process group: group zero broadcasts to
    // every attached process. A verified target PID alone does not establish
    // ownership of its console (a workload may attach to its parent's console).
    // Refuse any unknown member; the caller retains its owned-Job stop fallback.
    for member in members[..count]
        .iter()
        .copied()
        .filter(|member| *member != sender)
    {
        let Some(identity) = owned
            .iter()
            .find(|process| process.process_id == member)
            .and_then(|process| process.identity.as_ref())
        else {
            return Err(rejected(
                "console is shared with a process outside the managed workload",
            ));
        };
        verify_current_process_identity(member, identity)?;
    }
    Ok(())
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleProcessList(processes: *mut u32, capacity: u32) -> u32;
}

fn verify_current_process_identity(
    pid: u32,
    expected_identity: &ProcessIdentity,
) -> Result<(), RuntimeProcessError> {
    match inspect_process_identity(pid)? {
        None => Err(RuntimeProcessError::ProcessIdentityUnavailable { pid }),
        Some(actual) if !process_identities_match(expected_identity, &actual) => {
            Err(RuntimeProcessError::ProcessIdentityMismatch { pid })
        }
        Some(_) => Ok(()),
    }
}

pub(super) fn with_windows_console_ctrl_lock<T>(operation: impl FnOnce() -> T) -> T {
    let _guard = WINDOWS_CONSOLE_CTRL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

pub(super) trait WindowsConsoleControl {
    fn detach_console(&self) -> Result<(), std::io::Error>;
    fn attach_console(&self, pid: u32) -> Result<(), std::io::Error>;
    fn set_ctrl_c_ignored(&self, ignored: bool) -> Result<(), std::io::Error>;
    fn generate_ctrl_c(&self) -> Result<(), std::io::Error>;
}

struct NativeWindowsConsoleControl;

impl WindowsConsoleControl for NativeWindowsConsoleControl {
    fn detach_console(&self) -> Result<(), std::io::Error> {
        if unsafe { FreeConsole() } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn attach_console(&self, pid: u32) -> Result<(), std::io::Error> {
        if unsafe { AttachConsole(pid) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn set_ctrl_c_ignored(&self, ignored: bool) -> Result<(), std::io::Error> {
        let add_handler = if ignored { 1 } else { 0 };
        if unsafe { SetConsoleCtrlHandler(std::ptr::null_mut(), add_handler) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn generate_ctrl_c(&self) -> Result<(), std::io::Error> {
        if unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

struct WindowsConsoleCtrlSession<'a, C: WindowsConsoleControl> {
    control: &'a C,
    attached: bool,
    ctrl_c_ignored: bool,
}

impl<'a, C: WindowsConsoleControl> WindowsConsoleCtrlSession<'a, C> {
    fn attach(control: &'a C, pid: u32) -> Result<Self, std::io::Error> {
        control.attach_console(pid)?;
        Ok(Self {
            control,
            attached: true,
            ctrl_c_ignored: false,
        })
    }

    fn ignore_ctrl_c(&mut self) -> Result<(), std::io::Error> {
        self.control.set_ctrl_c_ignored(true)?;
        self.ctrl_c_ignored = true;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), (&'static str, std::io::Error)> {
        let mut first_error = None;

        if self.attached {
            match self.control.detach_console() {
                Ok(()) => self.attached = false,
                Err(error) => first_error = Some(("FreeConsole", error)),
            }
        }

        if self.ctrl_c_ignored {
            match self.control.set_ctrl_c_ignored(false) {
                Ok(()) => self.ctrl_c_ignored = false,
                Err(error) if first_error.is_none() => {
                    first_error = Some(("SetConsoleCtrlHandler(restore)", error));
                }
                Err(_) => {}
            }
        }

        first_error.map_or(Ok(()), Err)
    }
}

impl<C: WindowsConsoleControl> Drop for WindowsConsoleCtrlSession<'_, C> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
pub(super) fn request_windows_console_ctrl_c_with_control<C: WindowsConsoleControl>(
    control: &C,
    pid: u32,
    settle_delay: Duration,
) -> Result<(), std::io::Error> {
    let _ = control.detach_console();
    let mut session = WindowsConsoleCtrlSession::attach(control, pid)?;
    session.ignore_ctrl_c()?;
    control.generate_ctrl_c()?;
    std::thread::sleep(settle_delay);
    session.cleanup().map_err(|(_, source)| source)
}

pub(super) fn request_windows_console_ctrl_c_with_control_and_verifier<C, V>(
    control: &C,
    pid: u32,
    settle_delay: Duration,
    verify_after_attach: V,
) -> Result<(), RuntimeProcessError>
where
    C: WindowsConsoleControl,
    V: FnOnce() -> Result<(), RuntimeProcessError>,
{
    let _ = control.detach_console();
    let mut session = WindowsConsoleCtrlSession::attach(control, pid).map_err(|source| {
        RuntimeProcessError::ConsoleInterruptProcess {
            pid,
            operation: "AttachConsole",
            source,
        }
    })?;
    verify_after_attach()?;
    session
        .ignore_ctrl_c()
        .map_err(|source| RuntimeProcessError::ConsoleInterruptProcess {
            pid,
            operation: "SetConsoleCtrlHandler(ignore)",
            source,
        })?;
    control
        .generate_ctrl_c()
        .map_err(|source| RuntimeProcessError::ConsoleInterruptProcess {
            pid,
            operation: "GenerateConsoleCtrlEvent",
            source,
        })?;
    std::thread::sleep(settle_delay);
    session.cleanup().map_err(
        |(operation, source)| RuntimeProcessError::ConsoleInterruptProcess {
            pid,
            operation,
            source,
        },
    )
}

#[cfg(test)]
#[path = "windows_console_control_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "windows_console_spawn_tests.rs"]
mod spawn_tests;
