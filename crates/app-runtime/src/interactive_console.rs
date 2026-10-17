//! A short-lived console owned by its caller, outside instance logs and history.
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
use crate::{RuntimeChild, RuntimeStdinCancellation, WindowsHiddenDesktop};
#[cfg(windows)]
use std::time::Instant;

/// Windows ConPTY with an application-owned output sink and complete Job ownership.
///
/// Methods are blocking and belong on an owned worker. The sink must accept writes
/// promptly (normally a bounded memory sink), must protect sensitive output, and
/// must not persist it. Partial terminal rows are delivered as complete snapshots
/// after each read, so a later snapshot may repeat a prompt's prefix.
pub struct EphemeralConsole {
    #[cfg(windows)]
    child: Option<RuntimeChild>,
    #[cfg(windows)]
    desktop: Option<WindowsHiddenDesktop>,
    #[cfg(windows)]
    exit_code: Option<i32>,
}

impl EphemeralConsole {
    /// Launch directly inside a private ConPTY and desktop, without a shell or
    /// visible Windows console. Only `output` receives the terminal transcript.
    pub fn spawn(
        executable: &Path,
        arguments: &[String],
        working_directory: &Path,
        output: impl Write + Send + 'static,
    ) -> io::Result<Self> {
        #[cfg(not(windows))]
        {
            let _ = (executable, arguments, working_directory, output);
            Err(unsupported())
        }
        #[cfg(windows)]
        {
            if !executable.is_absolute() || !working_directory.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Interactive console executable and working directory must be absolute.",
                ));
            }
            let executable = executable.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Console executable must be Unicode.",
                )
            })?;
            let (child, desktop) = crate::pseudo_console::spawn_interactive(
                &crate::SpawnCommand {
                    executable,
                    args: arguments,
                    working_directory,
                    environment: &Default::default(),
                },
                output,
            )?;
            Ok(Self {
                child: Some(child),
                desktop,
                exit_code: None,
            })
        }
    }

    /// Submit one line, including an empty challenge response. No command or
    /// input history is retained here. Control characters and oversized lines
    /// are rejected by the shared bounded ConPTY writer.
    pub fn write_line(&mut self, input: &str) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            let _ = input;
            Err(unsupported())
        }
        #[cfg(windows)]
        {
            let Some(RuntimeChild::Windows(child)) = self.child.as_mut() else {
                return Err(closed());
            };
            if child
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal.output_failed())
            {
                return Err(output_failed());
            }
            child
                .stdin
                .as_mut()
                .ok_or_else(closed)?
                .write_stdin_line_with_cancellation(input, &RuntimeStdinCancellation::default())
        }
    }

    /// Inspect the root's exit code without releasing ownership of descendants
    /// or output. Call `terminate_and_wait` to confirm complete session cleanup.
    pub fn try_wait(&mut self) -> io::Result<Option<i32>> {
        #[cfg(not(windows))]
        {
            Err(unsupported())
        }
        #[cfg(windows)]
        {
            let Some(RuntimeChild::Windows(child)) = self.child.as_mut() else {
                return Ok(self.exit_code);
            };
            if child
                .terminal
                .as_ref()
                .is_some_and(|terminal| terminal.output_failed())
            {
                return Err(output_failed());
            }
            if let Some(status) = child.try_wait()? {
                self.exit_code = Some(status.code().ok_or_else(|| {
                    io::Error::other("The interactive console exited without an exit code.")
                })?);
            }
            Ok(self.exit_code)
        }
    }

    /// Terminate and reap the complete Job, then drain and close its console.
    /// Timeout/error retains the owner so the caller can retry cleanup. Closing
    /// the last owner still has the shared Job's kill-on-close fallback.
    pub fn terminate_and_wait(&mut self, budget: Duration) -> io::Result<()> {
        #[cfg(not(windows))]
        {
            let _ = budget;
            Err(unsupported())
        }
        #[cfg(windows)]
        {
            let deadline = Instant::now().checked_add(budget).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Console cleanup budget is too large.",
                )
            })?;
            let Some(RuntimeChild::Windows(child)) = self.child.as_mut() else {
                return Ok(());
            };
            child
                .job
                .as_ref()
                .ok_or_else(|| io::Error::other("Console Job ownership is unavailable."))?
                .terminate_and_wait_until(deadline)?;
            loop {
                if let Some(status) = child.try_wait()? {
                    self.exit_code = status.code();
                    break;
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Console process cleanup exceeded its deadline.",
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
            drop(child.stdin.take());
            if let Some(terminal) = child.terminal.as_mut() {
                terminal.finish_until(deadline)?;
            }
            drop(self.child.take());
            drop(self.desktop.take());
            Ok(())
        }
    }
}

impl Drop for EphemeralConsole {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.child.is_some() && self.terminate_and_wait(Duration::from_secs(7)).is_err() {
            // Preserve the final native fallback even if explicit bounded reaping
            // failed. Neither the input nor a caller-provided sink error is logged.
            drop(self.child.take());
            drop(self.desktop.take());
        }
    }
}

#[cfg(windows)]
fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "The interactive console is closed.",
    )
}

#[cfg(windows)]
fn output_failed() -> io::Error {
    io::Error::other("The interactive console output is incomplete; close the session.")
}

#[cfg(not(windows))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Interactive consoles require Windows.",
    )
}

#[cfg(test)]
#[path = "interactive_console_tests.rs"]
mod tests;
