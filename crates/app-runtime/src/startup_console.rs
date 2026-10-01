use std::fs::File;
use std::io;
#[cfg(not(windows))]
use std::io::Write;

use crate::SpawnedProcess;

/// A temporary writer for pre-registration probes sharing the child's stdin.
/// An incomplete write closes that shared input to prevent later command splicing.
/// Callers must allow at most one pending write and stop probing before publishing
/// the process to the supervisor.
pub struct StartupConsoleWriter {
    #[cfg(not(windows))]
    stdin: File,
    #[cfg(windows)]
    stdin: crate::WindowsPipeWriter<File>,
}

impl StartupConsoleWriter {
    pub fn write_line(&mut self, command: &str) -> io::Result<()> {
        #[cfg(windows)]
        return self
            .stdin
            .write_line(command, &crate::RuntimeStdinCancellation::default());
        #[cfg(not(windows))]
        {
            self.stdin.write_all(command.as_bytes())?;
            self.stdin.write_all(b"\n")?;
            self.stdin.flush()
        }
    }
}

impl SpawnedProcess {
    pub fn startup_console(&self) -> io::Result<StartupConsoleWriter> {
        // DST's Windows managed-terminal host supplies an ordinary pipe. Do not
        // duplicate ConPTY input: its framing and lifecycle have a separate owner.
        #[cfg(windows)]
        if let Some(crate::RuntimeChild::Windows(child)) = &self.child
            && let Some(crate::RuntimeStdin::Windows(stdin)) = &child.stdin
        {
            return Ok(StartupConsoleWriter {
                stdin: stdin.try_clone()?,
            });
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Process {} has no supported startup console pipe", self.pid),
        ))
    }
}
