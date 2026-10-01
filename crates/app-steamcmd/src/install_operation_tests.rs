use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

const LOCK_TEST_ROOT_ENV: &str = "LANGAME_STEAMCMD_LOCK_TEST_ROOT";
static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestChild(Option<Child>);

impl TestChild {
    fn spawn(root: &Path) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "install_operation::tests::hold_install_lock_in_child_process",
                "--nocapture",
            ])
            .env(LOCK_TEST_ROOT_ENV, root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self(Some(child))
    }

    fn try_wait(&mut self) -> Option<ExitStatus> {
        self.0.as_mut().unwrap().try_wait().unwrap()
    }

    fn kill_and_wait(&mut self) -> ExitStatus {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap()
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn test_lock_root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock")
        .as_nanos();
    let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "langame-install-lock-{label}-{}-{stamp}-{sequence}",
        std::process::id()
    ))
}

fn test_coordinator(root: &Path) -> InstallCoordinator {
    InstallCoordinator::with_lock_path(root.join(INSTALL_OPERATION_LOCK_FILE))
}

async fn wait_for_child_lock(child: &mut TestChild, ready_path: &Path) {
    for _ in 0..500 {
        if ready_path.exists() {
            return;
        }
        assert!(child.try_wait().is_none(), "lock child exited early");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("child did not acquire the OS lock");
}

async fn wait_for_child_exit(child: &mut TestChild) -> ExitStatus {
    for _ in 0..500 {
        if let Some(status) = child.try_wait() {
            child.0.take();
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("lock child did not exit");
}

#[tokio::test]
async fn coordinator_serializes_operations_and_releases_waiter() {
    let root = test_lock_root("process");
    let coordinator = test_coordinator(&root);
    let first = coordinator
        .acquire(InstallDeadline::new("first", Duration::from_secs(1)))
        .await
        .expect("first operation acquires coordinator");

    let waiting_coordinator = coordinator.clone();
    let waiter = tokio::spawn(async move {
        waiting_coordinator
            .acquire(InstallDeadline::new("waiter", Duration::from_secs(1)))
            .await
            .expect("waiting operation acquires after release")
    });

    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    drop(first);

    let second = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("waiter completes")
        .expect("waiter task succeeds");
    drop(second);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn coordinator_lock_wait_obeys_total_deadline() {
    let root = test_lock_root("deadline");
    let coordinator = test_coordinator(&root);
    let first = coordinator
        .acquire(InstallDeadline::new("first", Duration::from_secs(1)))
        .await
        .expect("first operation acquires coordinator");

    let result = coordinator
        .acquire(InstallDeadline::new("waiter", Duration::from_millis(30)))
        .await;

    assert!(matches!(result, Err(InstallAcquireError::Deadline)));
    drop(first);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancellation_releases_an_in_process_queue_waiter() {
    let root = test_lock_root("cancel-queue");
    let coordinator = test_coordinator(&root);
    let first = coordinator
        .acquire(InstallDeadline::new("holder", Duration::from_secs(5)))
        .await
        .unwrap();
    let cancellation = InstallCancellation::new();
    let waiting_token = cancellation.clone();
    let waiting_coordinator = coordinator.clone();
    let waiter = tokio::spawn(async move {
        waiting_token
            .scope(waiting_coordinator.acquire(InstallDeadline::new(
                "cancelled queue",
                Duration::from_secs(60),
            )))
            .await
    });
    tokio::task::yield_now().await;
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("queued cancellation does not wait for the holder")
        .unwrap();
    assert!(matches!(result, Err(InstallAcquireError::Deadline)));
    drop(first);
    let next = coordinator
        .acquire(InstallDeadline::new("next", Duration::from_secs(1)))
        .await
        .unwrap();
    drop(next);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancellation_joins_the_cross_process_lock_worker_before_returning() {
    let root = test_lock_root("cancel-os-lock");
    let holder = test_coordinator(&root)
        .acquire(InstallDeadline::new("holder", Duration::from_secs(5)))
        .await
        .unwrap();
    // Separate coordinators share only the OS file lock, not the Tokio gate.
    let coordinator = test_coordinator(&root);
    let waiting_coordinator = coordinator.clone();
    let cancellation = InstallCancellation::new();
    let waiting_token = cancellation.clone();
    let waiter = tokio::spawn(async move {
        waiting_token
            .scope(waiting_coordinator.acquire(InstallDeadline::new(
                "cancelled OS lock",
                Duration::from_secs(60),
            )))
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while coordinator.gate.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("waiter enters the cross-process lock stage");
    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("OS lock cancellation does not wait for the holder")
        .unwrap();
    assert!(matches!(result, Err(InstallAcquireError::Deadline)));
    drop(holder);
    let next = coordinator
        .acquire(InstallDeadline::new("next", Duration::from_secs(1)))
        .await
        .unwrap();
    drop(next);
    // On Windows a detached worker holding an open lock file prevents this.
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn lock_file_open_failure_is_reported_with_its_path() {
    let root = test_lock_root("open-error");
    let lock_path = root.join(INSTALL_OPERATION_LOCK_FILE);
    fs::create_dir_all(&lock_path).unwrap();
    let coordinator = InstallCoordinator::with_lock_path(lock_path.clone());

    let result = coordinator
        .acquire(InstallDeadline::new("open failure", Duration::from_secs(1)))
        .await;

    assert!(matches!(
        result,
        Err(InstallAcquireError::LockFile { path, .. }) if path == lock_path
    ));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cross_process_lock_waits_times_out_and_acquires_after_release() {
    let root = test_lock_root("cross-process");
    fs::create_dir_all(&root).unwrap();
    let ready_path = root.join("child-ready");
    let release_path = root.join("child-release");
    let mut child = TestChild::spawn(&root);
    wait_for_child_lock(&mut child, &ready_path).await;

    let coordinator = test_coordinator(&root);
    let timed_out = coordinator
        .acquire(InstallDeadline::new(
            "cross-process timeout",
            Duration::from_millis(120),
        ))
        .await;
    assert!(matches!(timed_out, Err(InstallAcquireError::Deadline)));

    let waiting_coordinator = coordinator.clone();
    let waiter = tokio::spawn(async move {
        waiting_coordinator
            .acquire(InstallDeadline::new(
                "cross-process waiter",
                Duration::from_secs(2),
            ))
            .await
    });
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert!(!waiter.is_finished());
    fs::write(&release_path, b"release").unwrap();
    let acquired = tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("waiter completes after release")
        .expect("waiter task succeeds")
        .expect("waiter acquires released OS lock");
    drop(acquired);

    assert!(wait_for_child_exit(&mut child).await.success());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn operating_system_releases_lock_when_holder_process_is_killed() {
    let root = test_lock_root("killed-holder");
    fs::create_dir_all(&root).unwrap();
    let ready_path = root.join("child-ready");
    let mut child = TestChild::spawn(&root);
    wait_for_child_lock(&mut child, &ready_path).await;

    let status = child.kill_and_wait();
    assert!(!status.success());
    let acquired = test_coordinator(&root)
        .acquire(InstallDeadline::new(
            "post-crash acquisition",
            Duration::from_secs(1),
        ))
        .await
        .expect("OS releases file lock when holder exits");
    drop(acquired);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn hold_install_lock_in_child_process() {
    let Some(root) = std::env::var_os(LOCK_TEST_ROOT_ENV).map(PathBuf::from) else {
        return;
    };
    let coordinator = test_coordinator(&root);
    let _guard = coordinator
        .acquire(InstallDeadline::new(
            "child lock holder",
            Duration::from_secs(5),
        ))
        .await
        .expect("child acquires OS lock");
    fs::write(root.join("child-ready"), b"ready").unwrap();
    for _ in 0..1_000 {
        if root.join("child-release").exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("parent process did not release child lock");
}
