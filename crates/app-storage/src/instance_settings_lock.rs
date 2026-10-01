use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::instance_creation_io::check_creation_cancelled;
use crate::{StorageError, StoragePaths};

const INSTANCE_SETTINGS_LOCK_DIRECTORY: &str = "instance-settings";
const MODULE_INSTANCE_CREATION_LOCK_DIRECTORY: &str = "module-instance-creation";

pub(crate) struct InstanceSettingsLock {
    lease: Arc<InstanceSettingsLockLease>,
}

struct InstanceSettingsLockLease {
    file: File,
}

impl Clone for InstanceSettingsLock {
    fn clone(&self) -> Self {
        Self {
            lease: Arc::clone(&self.lease),
        }
    }
}

impl InstanceSettingsLock {
    fn new(file: File) -> Self {
        Self {
            lease: Arc::new(InstanceSettingsLockLease { file }),
        }
    }

    /// A filesystem/database transaction must reach commit or compensation even
    /// when its caller stops waiting. Admission happens before spawning, and the
    /// task retains the same mutation lease until the entire operation finishes.
    pub(crate) async fn complete_mutation<F, T>(
        &self,
        operation: &'static str,
        mutation: F,
    ) -> Result<T, StorageError>
    where
        F: std::future::Future<Output = Result<T, StorageError>> + Send + 'static,
        T: Send + 'static,
    {
        let lease = self.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            let _lease = lease;
            if let Err(Err(error)) = sender.send(mutation.await) {
                eprintln!("{operation} failed after its caller stopped waiting: {error}");
            }
        });
        let result = receiver.await;
        worker
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation,
                message: error.to_string(),
            })?;
        result.map_err(|error| StorageError::BlockingTaskFailed {
            operation,
            message: error.to_string(),
        })?
    }

    /// Cancelling the waiter cannot release the lease while blocking filesystem
    /// work still reads or writes the instance. The last owner unlocks it.
    pub(crate) fn spawn_blocking<F, T>(&self, operation: F) -> tokio::task::JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let lease = self.clone();
        tokio::task::spawn_blocking(move || {
            let _lease = lease;
            operation()
        })
    }
}

pub(crate) fn acquire_instance_settings_mutation_lock(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceSettingsLock, StorageError> {
    acquire_lock_at_path(instance_settings_mutation_lock_path(paths, instance_id))
}

pub(crate) fn acquire_instance_settings_read_lock(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceSettingsLock, StorageError> {
    acquire_lock_at_path_with_mode(
        instance_settings_mutation_lock_path(paths, instance_id),
        LockMode::Shared,
    )
}

pub(crate) fn acquire_module_instance_creation_lock_blocking(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<InstanceSettingsLock, StorageError> {
    acquire_module_instance_creation_lock_cancellable(paths, module_id, None)
}

pub(crate) fn acquire_module_instance_creation_lock_cancellable(
    paths: &StoragePaths,
    module_id: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<InstanceSettingsLock, StorageError> {
    check_creation_cancelled(cancellation)?;
    let lock_path = module_instance_creation_lock_path(paths, module_id);
    let lock_directory = lock_path
        .parent()
        .expect("module instance creation lock path has a parent");
    fs::create_dir_all(lock_directory).map_err(|source| StorageError::CreatePath {
        path: lock_directory.to_path_buf(),
        source,
    })?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| StorageError::InstanceCreationLock {
            module_id: module_id.to_owned(),
            path: lock_path.clone(),
            source,
        })?;
    if let Some(cancellation) = cancellation {
        loop {
            check_creation_cancelled(Some(cancellation))?;
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    #[cfg(test)]
                    crate::instance_creation_io::test_gate::pause_if_registered(
                        cancellation,
                        crate::instance_creation_io::test_gate::PausePoint::Lock,
                    );
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(TryLockError::Error(source)) => {
                    return Err(StorageError::InstanceCreationLock {
                        module_id: module_id.to_owned(),
                        path: lock_path,
                        source,
                    });
                }
            }
        }
    } else {
        file.lock()
            .map_err(|source| StorageError::InstanceCreationLock {
                module_id: module_id.to_owned(),
                path: lock_path,
                source,
            })?;
    }
    check_creation_cancelled(cancellation)?;
    Ok(InstanceSettingsLock::new(file))
}

enum LockAcquireError {
    Contended,
    Io(io::Error),
}

enum LockMode {
    Shared,
    Exclusive,
}

fn acquire_lock_at_path(lock_path: PathBuf) -> Result<InstanceSettingsLock, StorageError> {
    acquire_lock_at_path_with_mode(lock_path, LockMode::Exclusive)
}

fn acquire_lock_at_path_with_mode(
    lock_path: PathBuf,
    mode: LockMode,
) -> Result<InstanceSettingsLock, StorageError> {
    let lock_directory = lock_path
        .parent()
        .expect("instance settings lock path has a parent");
    fs::create_dir_all(lock_directory).map_err(|source| StorageError::CreatePath {
        path: lock_directory.to_path_buf(),
        source,
    })?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| StorageError::InstanceSettingsLock {
            path: lock_path.clone(),
            source,
        })?;
    let result = map_try_lock_result(match mode {
        LockMode::Shared => file.try_lock_shared(),
        LockMode::Exclusive => file.try_lock(),
    });
    match result {
        Ok(()) => Ok(InstanceSettingsLock::new(file)),
        Err(LockAcquireError::Contended) => {
            Err(StorageError::InstanceSettingsLocked { path: lock_path })
        }
        Err(LockAcquireError::Io(source)) => Err(StorageError::InstanceSettingsLock {
            path: lock_path,
            source,
        }),
    }
}

fn map_try_lock_result(result: Result<(), TryLockError>) -> Result<(), LockAcquireError> {
    result.map_err(|error| match error {
        TryLockError::WouldBlock => LockAcquireError::Contended,
        TryLockError::Error(source) => LockAcquireError::Io(source),
    })
}

fn instance_settings_mutation_lock_path(paths: &StoragePaths, instance_id: &str) -> PathBuf {
    paths
        .instances_root
        .join(".langame")
        .join("locks")
        .join(INSTANCE_SETTINGS_LOCK_DIRECTORY)
        .join(format!("{}.lock", encode_lock_component(instance_id)))
}

fn module_instance_creation_lock_path(paths: &StoragePaths, module_id: &str) -> PathBuf {
    paths
        .instances_root
        .join(".langame")
        .join("locks")
        .join(MODULE_INSTANCE_CREATION_LOCK_DIRECTORY)
        .join(format!("{}.lock", encode_lock_component(module_id)))
}

fn encode_lock_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    if encoded.is_empty() {
        encoded.push_str("%00");
    }
    encoded
}

impl Drop for InstanceSettingsLockLease {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
#[path = "instance_settings_lock_worker_tests.rs"]
mod worker_tests;

#[cfg(test)]
#[path = "instance_settings_shared_lock_tests.rs"]
mod shared_read_tests;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Duration;

    use uuid::Uuid;

    use super::*;

    fn test_paths(root: &Path) -> StoragePaths {
        StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data").join("settings.json"),
            database_path: root.join("app-data").join("db").join("lgs.db"),
            logs_root: root.join("app-data").join("logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        }
    }

    #[test]
    fn mutation_locks_serialize_one_instance_without_blocking_another() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-instance-settings-lock-{}",
            Uuid::new_v4().simple()
        ));
        let paths = test_paths(&root);
        let first = acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
        let second = acquire_instance_settings_mutation_lock(&paths, "two").unwrap();
        assert!(matches!(
            acquire_instance_settings_mutation_lock(&paths, "one"),
            Err(StorageError::InstanceSettingsLocked { .. })
        ));
        drop(first);
        acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
        drop(second);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn instance_lock_contends_only_on_the_same_instance_across_processes() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-instance-settings-process-lock-{}",
            Uuid::new_v4().simple()
        ));
        let paths = test_paths(&root);
        fs::create_dir_all(&paths.instances_root).unwrap();
        let ready_path = root.join("child-ready");
        let release_path = root.join("child-release");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance_settings_lock::tests::hold_exclusive_lock_in_child_process",
                "--nocapture",
            ])
            .env("LSGM_LOCK_TEST_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        for _ in 0..500 {
            if ready_path.exists() {
                break;
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "lock child exited early"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let child_acquired_lock = ready_path.exists();
        let different_instance_is_available =
            acquire_instance_settings_mutation_lock(&paths, "different-instance").is_ok();
        let exclusive_is_blocked = matches!(
            acquire_instance_settings_mutation_lock(&paths, "locked-instance"),
            Err(StorageError::InstanceSettingsLocked { .. })
        );
        fs::write(&release_path, b"release").unwrap();
        let child_status = child.wait().unwrap();

        assert!(child_acquired_lock);
        assert!(different_instance_is_available);
        assert!(exclusive_is_blocked);
        assert!(child_status.success());
        acquire_instance_settings_mutation_lock(&paths, "locked-instance").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn per_instance_mutation_lock_is_visible_across_processes() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-instance-mutation-process-lock-{}",
            Uuid::new_v4().simple()
        ));
        let paths = test_paths(&root);
        fs::create_dir_all(&paths.instances_root).unwrap();
        let ready_path = root.join("instance-child-ready");
        let release_path = root.join("instance-child-release");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance_settings_lock::tests::hold_instance_mutation_lock_in_child_process",
                "--nocapture",
            ])
            .env("LSGM_INSTANCE_LOCK_TEST_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        for _ in 0..500 {
            if ready_path.exists() {
                break;
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "instance lock child exited early"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let child_acquired_lock = ready_path.exists();
        let same_instance_is_blocked = matches!(
            acquire_instance_settings_mutation_lock(&paths, "instance-one"),
            Err(StorageError::InstanceSettingsLocked { .. })
        );
        let other_instance_is_available =
            acquire_instance_settings_mutation_lock(&paths, "instance-two").is_ok();
        fs::write(&release_path, b"release").unwrap();
        let child_status = child.wait().unwrap();

        assert!(child_acquired_lock);
        assert!(same_instance_is_blocked);
        assert!(other_instance_is_available);
        assert!(child_status.success());
        acquire_instance_settings_mutation_lock(&paths, "instance-one").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn module_creation_lock_serializes_the_same_module_across_processes() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-module-creation-process-lock-{}",
            Uuid::new_v4().simple()
        ));
        let paths = test_paths(&root);
        fs::create_dir_all(&paths.instances_root).unwrap();
        let ready_path = root.join("module-child-ready");
        let release_path = root.join("module-child-release");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance_settings_lock::tests::hold_module_creation_lock_in_child_process",
                "--nocapture",
            ])
            .env("LSGM_MODULE_CREATION_LOCK_TEST_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        for _ in 0..500 {
            if ready_path.exists() {
                break;
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "module creation lock child exited early"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready_path.exists(), "child did not acquire module lock");

        let waiter_paths = paths.clone();
        let waiter = thread::spawn(move || {
            acquire_module_instance_creation_lock_blocking(&waiter_paths, "module-one")
        });
        thread::sleep(Duration::from_millis(50));
        assert!(
            !waiter.is_finished(),
            "the same module must wait for the cross-process lock"
        );
        let other_module =
            acquire_module_instance_creation_lock_blocking(&paths, "module-two").unwrap();

        fs::write(&release_path, b"release").unwrap();
        assert!(child.wait().unwrap().success());
        let same_module = waiter.join().unwrap().unwrap();
        drop(same_module);
        drop(other_module);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hold_exclusive_lock_in_child_process() {
        let Some(root) = std::env::var_os("LSGM_LOCK_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let paths = test_paths(&root);
        let _lock = acquire_instance_settings_mutation_lock(&paths, "locked-instance").unwrap();
        fs::write(root.join("child-ready"), b"ready").unwrap();
        for _ in 0..1_000 {
            if root.join("child-release").exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("parent process did not release lock child");
    }

    #[test]
    fn hold_instance_mutation_lock_in_child_process() {
        let Some(root) = std::env::var_os("LSGM_INSTANCE_LOCK_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let paths = test_paths(&root);
        let instance_id = std::env::var("LSGM_INSTANCE_LOCK_TEST_ID")
            .unwrap_or_else(|_| String::from("instance-one"));
        let _lock = acquire_instance_settings_mutation_lock(&paths, &instance_id).unwrap();
        fs::write(root.join("instance-child-ready"), b"ready").unwrap();
        for _ in 0..1_000 {
            if root.join("instance-child-release").exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("parent process did not release instance lock child");
    }

    #[test]
    fn hold_module_creation_lock_in_child_process() {
        let Some(root) = std::env::var_os("LSGM_MODULE_CREATION_LOCK_TEST_ROOT").map(PathBuf::from)
        else {
            return;
        };
        let paths = test_paths(&root);
        let _lock = acquire_module_instance_creation_lock_blocking(&paths, "module-one").unwrap();
        fs::write(root.join("module-child-ready"), b"ready").unwrap();
        for _ in 0..1_000 {
            if root.join("module-child-release").exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("parent process did not release module creation lock child");
    }

    #[test]
    fn instance_lock_component_encoding_cannot_escape_the_lock_directory() {
        let root = Path::new("storage");
        let paths = test_paths(root);
        let lock_path = instance_settings_mutation_lock_path(&paths, "../unsafe/instance%id");

        assert_eq!(
            lock_path.parent().unwrap(),
            paths
                .instances_root
                .join(".langame")
                .join("locks")
                .join(INSTANCE_SETTINGS_LOCK_DIRECTORY)
        );
        assert_eq!(
            lock_path.file_name().unwrap().to_string_lossy(),
            "%2E%2E%2Funsafe%2Finstance%25id.lock"
        );
    }
}
