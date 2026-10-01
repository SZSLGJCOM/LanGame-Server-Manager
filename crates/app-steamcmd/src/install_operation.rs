use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as RegistryMutex, OnceLock, Weak};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::time::{Instant, timeout_at};

use crate::{InstallCancellation, SteamCmdError};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

const INSTALL_OPERATION_LOCK_DIRECTORY: &str = "locks";
#[cfg(test)]
const INSTALL_OPERATION_LOCK_FILE: &str = "install-operation.lock";
const INSTALL_OPERATION_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Copy)]
pub(crate) struct InstallDeadline {
    operation: &'static str,
    timeout: Duration,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InstallDeadlineElapsed;

impl InstallDeadline {
    pub(crate) fn new(operation: &'static str, timeout: Duration) -> Self {
        Self {
            operation,
            timeout,
            expires_at: Instant::now() + timeout,
        }
    }

    pub(crate) fn operation(self) -> &'static str {
        self.operation
    }

    pub(crate) fn timeout(self) -> Duration {
        self.timeout
    }

    pub(crate) fn expires_at(self) -> Instant {
        self.expires_at
    }

    /// A phase or source attempt may shorten, but never extend, its owner.
    pub(crate) fn limited_to(self, budget: Duration) -> Self {
        Self {
            timeout: self.timeout.min(budget),
            expires_at: self.expires_at.min(Instant::now() + budget),
            ..self
        }
    }

    pub(crate) async fn run<T, F>(self, future: F) -> Result<T, InstallDeadlineElapsed>
    where
        F: Future<Output = T>,
    {
        let cancellation = InstallCancellation::current();
        tokio::select! {
            biased;
            _ = async {
                match cancellation.as_ref() {
                    Some(cancellation) => cancellation.cancelled().await,
                    None => std::future::pending().await,
                }
            } => Err(InstallDeadlineElapsed),
            result = timeout_at(self.expires_at, future) => {
                result.map_err(|_| InstallDeadlineElapsed)
            }
        }
    }

    pub(crate) fn check_cancelled(self) -> Result<(), SteamCmdError> {
        if InstallCancellation::current().is_some_and(|token| token.is_cancelled())
            || Instant::now() >= self.expires_at
        {
            Err(crate::operation_timeout(self))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InstallCoordinator {
    gate: Arc<Mutex<()>>,
    lock_path: Arc<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Debug)]
pub(crate) enum InstallAcquireError {
    Deadline,
    LockFile {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

pub(crate) struct InstallOperationGuard {
    _process_guard: Option<OwnedMutexGuard<()>>,
    lock_file: File,
}

impl InstallCoordinator {
    #[cfg(test)]
    pub(crate) fn with_lock_path(lock_path: PathBuf) -> Self {
        Self {
            gate: Arc::new(Mutex::new(())),
            lock_path: Arc::new(lock_path),
        }
    }

    #[cfg(test)]
    pub(crate) async fn acquire(
        &self,
        deadline: InstallDeadline,
    ) -> Result<InstallOperationGuard, InstallAcquireError> {
        self.acquire_mode(LockMode::Exclusive, deadline).await
    }

    pub(crate) fn for_resource(key: &str) -> Result<Self, InstallAcquireError> {
        type Gates = BTreeMap<String, Weak<Mutex<()>>>;
        static GATES: OnceLock<RegistryMutex<Gates>> = OnceLock::new();
        let directory = install_operation_data_root().join(INSTALL_OPERATION_LOCK_DIRECTORY);
        let mut gates = GATES
            .get_or_init(RegistryMutex::default)
            .lock()
            .map_err(|_| {
                lock_file_error(
                    "access installation resource gates",
                    &directory,
                    io::Error::other("installation resource gate registry is poisoned"),
                )
            })?;
        gates.retain(|_, gate| gate.strong_count() > 0);
        let gate = gates.get(key).and_then(Weak::upgrade).unwrap_or_else(|| {
            let gate = Arc::new(Mutex::new(()));
            gates.insert(key.to_owned(), Arc::downgrade(&gate));
            gate
        });
        let digest = Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self {
            gate,
            lock_path: Arc::new(directory.join(format!("resource-{digest}.lock"))),
        })
    }

    pub(crate) async fn acquire_mode(
        &self,
        mode: LockMode,
        deadline: InstallDeadline,
    ) -> Result<InstallOperationGuard, InstallAcquireError> {
        let process_guard = deadline
            .run(async {
                // Shared ancestor intents must not queue behind a waiting local
                // writer: a holder can need another root before releasing its
                // first root. OS shared locks preserve that acquisition order.
                match mode {
                    LockMode::Shared => None,
                    LockMode::Exclusive => Some(self.gate.clone().lock_owned().await),
                }
            })
            .await
            .map_err(|InstallDeadlineElapsed| InstallAcquireError::Deadline)?;
        let lock_path = self.lock_path.as_ref().clone();
        let worker_path = lock_path.clone();
        let cancellation = InstallCancellation::current();
        let abandoned = Arc::new(AtomicBool::new(false));
        let _wait = LockWait(abandoned.clone());
        let worker = tokio::task::spawn_blocking(move || {
            let lock_file = acquire_cross_process_lock(
                &worker_path,
                mode,
                deadline.expires_at(),
                cancellation.as_ref(),
                &abandoned,
            )?;
            Ok(InstallOperationGuard {
                _process_guard: process_guard,
                lock_file,
            })
        });
        // The bounded worker checks cancellation itself. Await it directly so
        // the process gate never releases while a detached worker can acquire
        // the OS lock on behalf of an operation that already returned.
        let operation = worker
            .await
            .map_err(|source| InstallAcquireError::LockFile {
                action: "join the cross-process install lock worker",
                path: lock_path,
                source: io::Error::other(format!("install lock worker failed: {source}")),
            })??;
        deadline
            .check_cancelled()
            .map_err(|_| InstallAcquireError::Deadline)?;

        Ok(operation)
    }
}

struct LockWait(Arc<AtomicBool>);
impl Drop for LockWait {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for InstallOperationGuard {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

fn acquire_cross_process_lock(
    lock_path: &Path,
    mode: LockMode,
    expires_at: Instant,
    cancellation: Option<&InstallCancellation>,
    abandoned: &AtomicBool,
) -> Result<File, InstallAcquireError> {
    let stopped = || {
        Instant::now() >= expires_at
            || cancellation.is_some_and(InstallCancellation::is_cancelled)
            || abandoned.load(Ordering::Acquire)
    };
    if stopped() {
        return Err(InstallAcquireError::Deadline);
    }
    let Some(lock_directory) = lock_path.parent() else {
        return Err(lock_file_error(
            "validate the cross-process install lock path",
            lock_path,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "install lock path has no parent",
            ),
        ));
    };
    fs::create_dir_all(lock_directory).map_err(|source| {
        lock_file_error(
            "create the cross-process install lock directory",
            lock_directory,
            source,
        )
    })?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    let file = options.open(lock_path).map_err(|source| {
        lock_file_error(
            "open the cross-process install lock file",
            lock_path,
            source,
        )
    })?;

    loop {
        if stopped() {
            return Err(InstallAcquireError::Deadline);
        }
        let acquired = match mode {
            LockMode::Shared => file.try_lock_shared(),
            LockMode::Exclusive => file.try_lock(),
        };
        match acquired {
            Ok(()) if !stopped() => return Ok(file),
            Ok(()) => {
                let _ = file.unlock();
                return Err(InstallAcquireError::Deadline);
            }
            Err(TryLockError::WouldBlock) => {
                let remaining = expires_at.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(InstallAcquireError::Deadline);
                }
                std::thread::sleep(remaining.min(INSTALL_OPERATION_LOCK_RETRY_INTERVAL));
            }
            Err(TryLockError::Error(source)) => {
                return Err(lock_file_error(
                    "acquire the cross-process install lock",
                    lock_path,
                    source,
                ));
            }
        }
    }
}

fn lock_file_error(
    action: &'static str,
    lock_path: &Path,
    source: io::Error,
) -> InstallAcquireError {
    InstallAcquireError::LockFile {
        action,
        path: lock_path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
fn install_operation_data_root() -> PathBuf {
    // Test children inherit the managed temporary root. Keep disposable fixture
    // identities out of the user's persistent lifecycle lock directory.
    // The managed cleaner temporarily lengthens the scratch directory name.
    // Leave room for that rename and the SHA-256 lock filename on Windows.
    std::env::temp_dir().join("lg")
}

#[cfg(not(test))]
fn install_operation_data_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|root| root.join(".local").join("share"))
        })
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|root| root.join("AppData").join("Local"))
        })
        .unwrap_or_else(std::env::temp_dir)
        .join("LanGame")
        .join("ServerManager")
}

#[cfg(test)]
#[path = "install_operation_tests.rs"]
mod tests;
