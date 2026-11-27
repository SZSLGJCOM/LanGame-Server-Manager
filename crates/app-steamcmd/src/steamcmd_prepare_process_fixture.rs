#![cfg(test)]

use std::fs;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use super::*;
use crate::{apply_no_window, ps_literal};
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};

pub(super) struct PreparedProcess {
    child: Child,
    guard: ChildProcessGuard,
    process: OwnedHandle,
}

impl PreparedProcess {
    pub(super) async fn start(root: &Path, setup: &str, body: &str) -> Self {
        let ready = root.join("fixture-ready");
        let release = root.join("fixture-release");
        let script = format!(
            concat!(
                "$ErrorActionPreference='Stop'; {setup}; ",
                "[IO.File]::WriteAllText({ready}, 'ready'); ",
                "while (-not [IO.File]::Exists({release})) {{ Start-Sleep -Milliseconds 10 }}; ",
                "{body}"
            ),
            setup = setup,
            body = body,
            ready = ps_literal(&ready),
            release = ps_literal(&release),
        );
        let mut command = Command::new("powershell");
        apply_no_window(&mut command);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let (mut child, mut guard) = crate::spawn_managed_command(&mut command).await.unwrap();
        let process = open_process(child.id().unwrap());
        // PowerShell startup is fixture setup, not the idle/total-timeout
        // behavior under test. Keep the existing bounded 20s handshake.
        let readiness = tokio::time::timeout(Duration::from_secs(20), async {
            while !ready.exists() {
                if child.try_wait()?.is_some() {
                    return Err(std::io::Error::other("fixture exited before readiness"));
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, std::io::Error>(())
        })
        .await;
        if !matches!(readiness, Ok(Ok(()))) {
            terminate_and_reap_preparation_tree(&mut child, &mut guard)
                .await
                .unwrap();
            panic!("fixture must finish its setup handshake: {readiness:?}");
        }
        assert_running(&process);
        eprintln!("preparation fixture ready after {:?}", started.elapsed());
        fs::write(&release, b"start behavior").unwrap();
        Self {
            child,
            guard,
            process,
        }
    }

    pub(super) async fn capture<F: FnMut(SteamCmdPrepareProgress)>(
        self,
        deadline: InstallDeadline,
        idle: Duration,
        handoff: bool,
        reporter: &mut PrepareReporter<F>,
    ) -> (Result<PreparationAttempt, SteamCmdError>, OwnedHandle) {
        let result = capture_preparation_process(
            self.child, self.guard, None, deadline, idle, handoff, reporter,
        )
        .await;
        (result, self.process)
    }
}

pub(super) fn open_process(pid: u32) -> OwnedHandle {
    const SYNCHRONIZE: u32 = 0x00100000;
    let raw = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    assert!(
        !raw.is_null(),
        "open owned process {pid}: {}",
        std::io::Error::last_os_error()
    );
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

pub(super) fn assert_running(process: &OwnedHandle) {
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) },
        WAIT_TIMEOUT,
        "fixture process must be alive before the behavior under test"
    );
}

pub(super) fn exited_code(process: &OwnedHandle) -> u32 {
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) },
        WAIT_OBJECT_0,
        "owned fixture process survived preparation cleanup"
    );
    let mut code = 0;
    assert_ne!(
        unsafe { GetExitCodeProcess(process.as_raw_handle().cast(), &mut code) },
        0
    );
    code
}
