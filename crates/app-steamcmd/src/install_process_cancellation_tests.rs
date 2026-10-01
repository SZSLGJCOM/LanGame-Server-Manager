#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::{InstallCancellation, ps_literal};
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-cancel-process-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("child.ps1"),
            format!(
                "[IO.File]::WriteAllText({}, [string]$PID); Start-Sleep -Seconds 30",
                ps_literal(&root.join("child-pid"))
            ),
        )
        .unwrap();
        fs::write(
            root.join("parent.ps1"),
            format!(
                concat!(
                    "[IO.File]::WriteAllText({parent}, [string]$PID); ",
                    "$info=New-Object Diagnostics.ProcessStartInfo; $info.FileName='powershell.exe'; ",
                    "$info.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"' + {script} + '\"'; ",
                    "$info.UseShellExecute=$false; $info.CreateNoWindow=$true; ",
                    "$child=[Diagnostics.Process]::Start($info); ",
                    "while (-not [IO.File]::Exists({child})) {{ Start-Sleep -Milliseconds 10 }}; ",
                    "[Console]::WriteLine('Waiting for client config... fixture running'); Start-Sleep -Seconds 30"
                ),
                parent = ps_literal(&root.join("parent-pid")),
                script = ps_literal(&root.join("child.ps1")),
                child = ps_literal(&root.join("child-pid")),
            ),
        )
        .unwrap();
        Self(root)
    }

    fn command(&self) -> Command {
        let mut command = Command::new("powershell.exe");
        apply_no_window(&mut command);
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(self.0.join("parent.ps1"));
        command
    }

    fn assert_owned_processes_exited(&self) {
        for file in ["parent-pid", "child-pid"] {
            assert_process_exited(&self.0.join(file));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_process_exited(pid_path: &Path) {
    let pid: u32 = fs::read_to_string(pid_path).unwrap().parse().unwrap();
    let raw = unsafe {
        // SAFETY: the PID comes from this test's owned child process.
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
    };
    if raw.is_null() {
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(ERROR_INVALID_PARAMETER as i32),
            "unable to inspect owned process {pid}"
        );
        return;
    }
    let process = unsafe {
        // SAFETY: OpenProcess returned one owned process handle.
        OwnedHandle::from_raw_handle(raw)
    };
    let mut exit_code = 0;
    let queried = unsafe {
        // SAFETY: the process handle and exit-code buffer remain valid.
        GetExitCodeProcess(process.as_raw_handle().cast(), &mut exit_code)
    };
    assert_ne!(queried, 0);
    assert_ne!(
        exit_code, STILL_ACTIVE as u32,
        "owned process {pid} survived cancellation"
    );
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("process cancellation fixture", Duration::from_secs(20))
}

#[tokio::test]
async fn capture_cancellation_waits_for_owned_parent_descendant_and_readers() {
    let fixture = Fixture::new();
    let cancellation = InstallCancellation::new();
    let mut capture =
        Box::pin(cancellation.scope(run_command_capture(fixture.command(), deadline())));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fixture.0.join("child-pid").exists() {
            tokio::select! {
                _ = &mut capture => panic!("fixture must remain running"),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await
    .expect("owned descendant starts");
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(8), capture)
        .await
        .expect("cancellation cleans up the owned process tree");
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_owned_processes_exited();
}

#[tokio::test]
async fn streaming_cancellation_reaps_owned_tree_before_returning() {
    let fixture = Fixture::new();
    let launcher = fixture.0.join("steamcmd.cmd");
    fs::write(
        &launcher,
        format!(
            "@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"\r\n",
            fixture.0.join("parent.ps1").display()
        ),
    )
    .unwrap();
    let cancellation = InstallCancellation::new();
    let token = cancellation.clone();
    let result = cancellation
        .scope(crate::steamcmd_stream::run_steamcmd_script_with_progress(
            launcher.to_str().unwrap(),
            &fixture.0.join("ignored-script.txt"),
            fixture.0.to_str().unwrap(),
            deadline(),
            move |progress| {
                if progress.output_excerpt.contains("fixture running") {
                    token.cancel();
                }
            },
        ))
        .await;
    assert!(matches!(
        result,
        Err(SteamCmdError::InstallCancelled { .. })
    ));
    fixture.assert_owned_processes_exited();
}

#[tokio::test]
async fn startup_failure_reaps_the_owned_tree_before_returning() {
    let fixture = Fixture::new();
    let mut command = fixture.command();
    command.stdout(std::process::Stdio::null());
    let (mut child, mut guard) = spawn_managed_command(&mut command).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fixture.0.join("child-pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned descendant starts");
    let cancellation = InstallCancellation::new();
    cancellation.cancel();
    let error = cancellation
        .scope(reap_failed_command_start(
            &mut child,
            &mut guard,
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture startup failure",
            ),
        ))
        .await;
    let ManagedCommandSpawnError::ProcessManagement(SteamCmdError::SpawnCommand { source }) = error
    else {
        panic!("a reaped startup failure must retain its original cause");
    };
    assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(source.to_string(), "fixture startup failure");
    fixture.assert_owned_processes_exited();
}
