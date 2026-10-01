use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::ps_literal;
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};

const STARTUP_BUDGET: Duration = Duration::from_secs(20);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(2);

struct Fixture {
    root: PathBuf,
    ready: PathBuf,
    release: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new(work_duration: Duration) -> Self {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(&root).expect("create deadline fixture");
        let fixture = Self {
            ready: root.join("ready-pid.txt"),
            release: root.join("release.txt"),
            marker: root.join("completed.txt"),
            root,
        };
        fs::write(
            fixture.root.join("child.ps1"),
            format!(
                concat!(
                    "[IO.File]::WriteAllText({ready}, [string]$PID)\n",
                    "while (-not [IO.File]::Exists({release})) {{ Start-Sleep -Milliseconds 10 }}\n",
                    "Start-Sleep -Milliseconds {work_millis}\n",
                    "[IO.File]::WriteAllText({marker}, 'completed')\n",
                    "[Console]::WriteLine('child completed')\n"
                ),
                ready = ps_literal(&fixture.ready),
                release = ps_literal(&fixture.release),
                marker = ps_literal(&fixture.marker),
                work_millis = work_duration.as_millis(),
            ),
        )
        .expect("write child fixture");
        fs::write(
            fixture.root.join("parent.ps1"),
            format!(
                concat!(
                    "$childArgs = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"' + {child} + '\"'\n",
                    "$child = Start-Process -FilePath 'powershell.exe' -ArgumentList $childArgs -NoNewWindow -PassThru\n",
                    "while (-not [IO.File]::Exists({ready})) {{ Start-Sleep -Milliseconds 10 }}\n",
                    "exit 0\n"
                ),
                child = ps_literal(&fixture.root.join("child.ps1")),
                ready = ps_literal(&fixture.ready),
            ),
        )
        .expect("write parent fixture");
        fixture
    }

    async fn start_ready(&self, script: &str) -> (Child, ChildProcessGuard, OwnedHandle) {
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
            .arg(self.root.join(script))
            .current_dir(&self.root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let (mut child, mut guard) = spawn_managed_command(&mut command)
            .await
            .expect("spawn owned fixture");
        let ready = tokio::time::timeout(STARTUP_BUDGET, async {
            loop {
                if let Ok(value) = fs::read_to_string(&self.ready)
                    && let Ok(pid) = value.trim().parse::<u32>()
                {
                    const SYNCHRONIZE: u32 = 0x00100000;
                    let handle = unsafe {
                        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid)
                    };
                    if handle.is_null() {
                        return Err(std::io::Error::last_os_error());
                    }
                    // Retain the actual process object through the assertion;
                    // a later process reusing its PID cannot satisfy the check.
                    let process = unsafe { OwnedHandle::from_raw_handle(handle) };
                    if unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) }
                        != WAIT_TIMEOUT
                    {
                        return Err(std::io::Error::other("fixture exited before release"));
                    }
                    return Ok(process);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        match ready {
            Ok(Ok(process)) => {
                let mut member = 0;
                let job = guard.job.as_ref().expect("Windows fixture owns a Job");
                let queried = unsafe {
                    IsProcessInJob(
                        process.as_raw_handle().cast(),
                        job.handle.as_raw_handle().cast(),
                        &mut member,
                    )
                };
                if queried == 0 || member == 0 {
                    terminate_and_reap_preparation_tree(&mut child, &mut guard)
                        .await
                        .expect("clean up fixture with missing membership");
                    panic!("ready process must belong to the exact fixture Job");
                }
                (child, guard, process)
            }
            result => {
                terminate_and_reap_preparation_tree(&mut child, &mut guard)
                    .await
                    .expect("clean up failed fixture readiness");
                panic!("owned fixture must complete its startup handshake: {result:?}");
            }
        }
    }

    fn release(&self) {
        fs::write(&self.release, b"run").expect("release ready fixture");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_exited(process: &OwnedHandle) {
    assert_eq!(
        unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) },
        WAIT_OBJECT_0,
        "owned process must have exited before timeout cleanup returns"
    );
}

async fn await_root_exit(child: &mut Child, guard: &mut ChildProcessGuard) {
    let result = tokio::time::timeout(STARTUP_BUDGET, child.wait()).await;
    if !matches!(&result, Ok(Ok(status)) if status.success()) {
        terminate_and_reap_preparation_tree(child, guard)
            .await
            .expect("clean up parent startup failure");
        panic!("root must exit before the descendant operation begins: {result:?}");
    }
}

#[tokio::test]
async fn powershell_deadline_terminates_and_reaps_timed_out_process() {
    let fixture = Fixture::new(Duration::from_secs(5));
    let (child, guard, process) = fixture.start_ready("child.ps1").await;
    // Interpreter startup has its own bounded handshake. The business deadline
    // starts only once the intended process is ready, even under parallel load.
    let deadline = InstallDeadline::new("timeout process test", OPERATION_TIMEOUT);
    let (result, ()) = tokio::join!(biased;
        capture_spawned_command(child, guard, deadline),
        async { fixture.release(); },
    );
    let error = result.expect_err("sleeping process must hit deadline");
    assert!(matches!(
        error,
        SteamCmdError::OperationTimedOut {
            operation: "timeout process test",
            timeout_seconds: 2
        }
    ));
    assert_exited(&process);
    assert!(!fixture.marker.exists());
}

#[tokio::test]
async fn deadline_job_kills_grandchild_after_root_process_exits() {
    let fixture = Fixture::new(Duration::from_secs(5));
    let (mut child, mut guard, process) = fixture.start_ready("parent.ps1").await;
    await_root_exit(&mut child, &mut guard).await;
    let deadline = InstallDeadline::new("grandchild timeout test", OPERATION_TIMEOUT);
    let (result, ()) = tokio::join!(biased;
        capture_spawned_command(child, guard, deadline),
        async { fixture.release(); },
    );
    let error = result.expect_err("live grandchild must hold the operation until its deadline");
    assert!(matches!(
        error,
        SteamCmdError::OperationTimedOut {
            operation: "grandchild timeout test",
            timeout_seconds: 2
        }
    ));
    assert_exited(&process);
    assert!(!fixture.marker.exists());
}

#[tokio::test]
async fn completed_process_tree_returns_well_before_deadline() {
    let fixture = Fixture::new(Duration::ZERO);
    let (mut child, mut guard, process) = fixture.start_ready("parent.ps1").await;
    await_root_exit(&mut child, &mut guard).await;
    let deadline = InstallDeadline::new("completed tree test", Duration::from_secs(5));
    let started = std::time::Instant::now();
    let (result, ()) = tokio::join!(biased;
        capture_spawned_command(child, guard, deadline),
        async { fixture.release(); },
    );
    let output = result.expect("completed parent and child return successfully");
    assert!(output.status.success());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(fs::read_to_string(&fixture.marker).unwrap(), "completed");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("child completed")
    );
    assert_exited(&process);
}
