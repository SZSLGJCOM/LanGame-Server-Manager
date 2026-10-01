use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::path::PathBuf;

use crate::{RuntimeChild, SpawnCommand, WindowsHiddenDesktop, WindowsProcessHandle};

pub(super) struct LaunchRequest {
    executable: String,
    args: Vec<String>,
    directory: PathBuf,
    stdout: usize,
    stderr: usize,
    background: bool,
}

impl LaunchRequest {
    pub(super) fn parse(value: &serde_json::Value) -> io::Result<Self> {
        let invalid = || super::invalid("invalid elevated launch request");
        let object = value.as_object().ok_or_else(invalid)?;
        if object.len() != 6 {
            return Err(invalid());
        }
        let executable = value["executable"].as_str().ok_or_else(invalid)?.to_owned();
        let directory = PathBuf::from(value["directory"].as_str().ok_or_else(invalid)?);
        let args = value["args"].as_array().ok_or_else(invalid)?;
        if args.len() > 256 {
            return Err(invalid());
        }
        let args = args
            .iter()
            .map(|arg| arg.as_str().map(str::to_owned).ok_or_else(invalid))
            .collect::<io::Result<Vec<_>>>()?;
        let stdout = value["stdout"]
            .as_u64()
            .and_then(|handle| usize::try_from(handle).ok())
            .filter(|handle| *handle != 0)
            .ok_or_else(invalid)?;
        let stderr = value["stderr"]
            .as_u64()
            .and_then(|handle| usize::try_from(handle).ok())
            .filter(|handle| *handle != 0)
            .ok_or_else(invalid)?;
        let background = value["background"].as_bool().ok_or_else(invalid)?;
        if !std::path::Path::new(&executable).is_absolute()
            || !directory.is_absolute()
            || !std::path::Path::new(&executable)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            || executable.contains('\0')
            || directory.to_string_lossy().contains('\0')
            || args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(invalid());
        }
        Ok(Self {
            executable,
            args,
            directory,
            stdout,
            stderr,
            background,
        })
    }

    pub(super) fn start(
        &self,
        parent: &WindowsProcessHandle,
    ) -> io::Result<(RuntimeChild, Option<WindowsHiddenDesktop>)> {
        let stdout = duplicate_output(parent, self.stdout)?;
        let stderr = duplicate_output(parent, self.stderr)?;
        let environment = BTreeMap::new();
        let plan = SpawnCommand {
            executable: &self.executable,
            args: &self.args,
            working_directory: &self.directory,
            environment: &environment,
        };
        // Pin and check the original process object immediately before creation.
        // Death in the remaining narrow interval is caught by the cleanup loop;
        // the game's Job is attached atomically by CreateProcess attributes.
        if !parent.is_running().map_err(io::Error::other)? {
            return Err(super::denied(
                "launcher parent exited before process creation",
            ));
        }
        crate::windows_process_spawn::spawn_elevated_workload(
            &plan,
            stdout,
            stderr,
            self.background,
        )
    }
}

fn duplicate_output(parent: &WindowsProcessHandle, source: usize) -> io::Result<File> {
    let mut raw = std::ptr::null_mut();
    // Preserve the parent's append-only file rights. The helper never opens a
    // parent-selected path under its elevated token or adds new access rights.
    if unsafe {
        crate::DuplicateHandle(
            parent.raw,
            source as *mut _,
            crate::GetCurrentProcess(),
            &mut raw,
            0,
            0,
            crate::DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(raw) };
    if unsafe { windows_sys::Win32::Storage::FileSystem::GetFileType(raw) }
        != windows_sys::Win32::Storage::FileSystem::FILE_TYPE_DISK
    {
        return Err(super::invalid(
            "elevated output must be an existing disk file",
        ));
    }
    Ok(file)
}
